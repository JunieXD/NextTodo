import Capacitor
import QuickLook
import UIKit
import UniformTypeIdentifiers

/// What to tell the user of `e`, which made `what` fail.
func forUser(_ what: String, _ e: Error) -> String {
    switch e {
    case let e as Failure: return e.message
    case let e as Local.StartFailed: return e.message
    case let e as HttpError: return "\(what)失败：\(e.message)"
    case is URLError: return "\(what)失败：无法连接，请检查网络后重试"
    default:
        NSLog("%@: %@", what, String(describing: e))
        return "\(what)失败，请稍后重试"
    }
}

/// What the web app asks of iOS beyond the stock plugins: the methods of `apps/web/src/mobile.ts` that
/// iOS has (docs/ios.md#结构).
@objc(NextTodoPlugin)
public class NextTodoPlugin: CAPPlugin, CAPBridgedPlugin, UIDocumentPickerDelegate, QLPreviewControllerDataSource {
    public let identifier = "NextTodoPlugin"
    public let jsName = "NextTodo"
    public let pluginMethods: [CAPPluginMethod] = [
        "setReminders", "clearReminders", "reminderSettings", "allowNotifications", "signIn", "takeChanges", "forgetChanges",
        "openAppSettings", "appInfo", "openLocal", "startLocal", "setLocalMode", "restart", "backupLocal", "prepareRestore",
        "commitRestore", "writeAttachment", "openAttachment", "saveAttachment",
    ].map { CAPPluginMethod(name: $0, returnType: CAPPluginReturnPromise)! }

    /// In the temporary directory, the attachments' files written to be opened or saved.
    private static let attachments = FileManager.default.temporaryDirectory.appendingPathComponent("attachments")
    /// Writing the attachments' files, one at a time.
    private let files = DispatchQueue(label: "cn.juniexd.nexttodo.attachments")
    /// Who hears what the user picks in the document picker showing; on the main thread.
    private var picked: ((URL?) -> Void)?
    /// The file the preview showing has.
    private var previewing: URL?

    /// The attachments opened in the last run go as the page loads.
    override public func load() {
        try? FileManager.default.removeItem(at: Self.attachments)
    }

    /// Does `work`, which is to `what`, on `queue`: resolves `call` with what it gives, or rejects it
    /// with what to tell the user.
    private func work(_ call: CAPPluginCall, _ what: String, on queue: DispatchQueue, _ work: @escaping () throws -> [String: Any]) {
        queue.async {
            do {
                call.resolve(try work())
            } catch {
                call.reject(forUser(what, error))
            }
        }
    }

    /// Hands over the reminders, as JSON in the shape `apps/web/src/mobile.ts` gives
    /// (docs/ios.md#截止提醒). The first time there are some, asks for notifications.
    @objc func setReminders(_ call: CAPPluginCall) {
        work(call, "设置提醒", on: Reminders.queue) {
            try Reminders.set(call.getString("reminders")!, digest: call.getString("digest")!, seq: Int64(call.getInt("seq")!))
            return [:]
        }
    }

    @objc func clearReminders(_ call: CAPPluginCall) {
        work(call, "清除提醒", on: Reminders.queue) {
            Reminders.clear()
            return [:]
        }
    }

    /// Whether notifications are allowed; on iOS they come on time, without a card.
    @objc func reminderSettings(_ call: CAPPluginCall) {
        work(call, "读取通知设置", on: Reminders.queue) {
            let status = Reminders.settings().authorizationStatus
            return ["notify": status != .denied && status != .notDetermined, "exact": true, "card": false, "overlay": false]
        }
    }

    @objc func allowNotifications(_ call: CAPPluginCall) {
        work(call, "请求通知权限", on: Reminders.queue) { ["notify": Reminders.allow()] }
    }

    /// Signing in as `user`, before the token is written: the changes made natively for anyone else go.
    @objc func signIn(_ call: CAPPluginCall) {
        work(call, "删除提醒里其他账号的改动", on: Changes.queue) {
            Changes.signIn(user: call.getString("user")!)
            return [:]
        }
    }

    /// The changes made natively for `user` and not sent yet, as JSON `[{id, ops}]`, which the page
    /// takes in; anyone else's go (docs/ios.md#原生发出的改动).
    @objc func takeChanges(_ call: CAPPluginCall) {
        work(call, "读取提醒里的改动", on: Changes.queue) { ["changes": Changes.takeOver(user: call.getString("user")!)] }
    }

    /// Signed out: the changes made natively and not sent yet go.
    @objc func forgetChanges(_ call: CAPPluginCall) {
        work(call, "删除提醒里的改动", on: Changes.queue) {
            Changes.forget()
            return [:]
        }
    }

    /// The app's notification settings: all that the page sends the user there for on iOS.
    @objc func openAppSettings(_ call: CAPPluginCall) {
        DispatchQueue.main.async {
            UIApplication.shared.open(URL(string: UIApplication.openNotificationSettingsURLString)!)
            call.resolve()
        }
    }

    /// This build's version, and its build number, the count of commits.
    @objc func appInfo(_ call: CAPPluginCall) {
        let info = Bundle.main.infoDictionary!
        call.resolve(["version": info["CFBundleShortVersionString"]!, "native": Int(info["CFBundleVersion"] as! String)!])
    }

    /// Local mode (`Local`): the server's address, the session's token, the local user and the epoch.
    /// Starts the server, should it not run, or not be at its address any more; rejects with code `locked` while it still lets go of its
    /// data, having just stopped.
    @objc func openLocal(_ call: CAPPluginCall) {
        Local.queue.async {
            do {
                call.resolve(try Local.open())
            } catch let e as Local.StartFailed {
                call.reject(e.message, e.locked ? "locked" : nil)
            } catch {
                call.reject(forUser("打开本机模式", error))
            }
        }
    }

    /// Starts local mode's server here, to enter it: the local user, whose data it has.
    @objc func startLocal(_ call: CAPPluginCall) {
        work(call, "启动本机模式", on: Local.queue) { ["user": try Local.start()["user"]!] }
    }

    /// Enters local mode, or leaves it, from the page's next load: see `restart`.
    @objc func setLocalMode(_ call: CAPPluginCall) {
        work(call, "切换本机模式", on: Local.queue) {
            Local.set(on: call.getBool("on")!)
            return [:]
        }
    }

    /// What starting the app afresh does elsewhere: the server stops, and the page loads again.
    @objc func restart(_ call: CAPPluginCall) {
        let page = bridge?.viewController as? CAPBridgeViewController
        Local.queue.async {
            Local.stop()
            call.resolve()
            DispatchQueue.main.async { page?.loadWebView() }
        }
    }

    /// Writes a backup named `name` and asks where to save it: `saved`, the file's name, or nothing when
    /// the user cancelled. Through a file in the temporary directory, which the server writes.
    @objc func backupLocal(_ call: CAPPluginCall) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let file = dir.appendingPathComponent(call.getString("name")!)
        Local.queue.async {
            do {
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                try Local.backup(to: file)
            } catch {
                return call.reject(forUser("备份", error))
            }
            self.pick(UIDocumentPickerViewController(forExporting: [file], asCopy: true)) { saved in
                try? FileManager.default.removeItem(at: dir)
                call.resolve(saved.map { ["saved": $0.lastPathComponent] } ?? [:])
            }
        }
    }

    /// Asks for a backup, and prepares it to replace the data: `picked`, whether the user picked one.
    /// Works without the server, which may have failed to start.
    @objc func prepareRestore(_ call: CAPPluginCall) {
        pick(UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: true)) { file in
            guard let file else { return call.resolve(["picked": false]) }
            self.work(call, "读取备份", on: Local.queue) {
                defer { try? FileManager.default.removeItem(at: file) }
                try Local.prepare(from: file)
                return ["picked": true]
            }
        }
    }

    /// The user's go-ahead: the backup prepared replaces the data as the page loads next.
    @objc func commitRestore(_ call: CAPPluginCall) {
        work(call, "恢复", on: Local.queue) {
            try Local.commit()
            return [:]
        }
    }

    /// The file of the attachment `call` names, under the name it goes by: the id read as one, a single
    /// part of the path whatever the page sends.
    private func attachment(_ call: CAPPluginCall) throws -> URL {
        let id = UUID(uuidString: call.getString("id")!)!.uuidString.lowercased()
        return Self.attachments.appendingPathComponent(id).appendingPathComponent(try LocalServer.fileName(call.getString("name")!))
    }

    /// Writes the `first` piece of an attachment's file, or the next, base64 `data`: the bridge takes
    /// no whole file (docs/android.md#附件). Beside the file until the `last`, which puts it in place
    /// whole.
    @objc func writeAttachment(_ call: CAPPluginCall) {
        work(call, "读取附件", on: files) {
            let file = try self.attachment(call)
            let part = file.deletingLastPathComponent().appendingPathExtension("part")
            let data = Data(base64Encoded: call.getString("data")!)!
            if call.getBool("first")! {
                try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
                try data.write(to: part)
            } else {
                let handle = try FileHandle(forWritingTo: part)
                defer { try? handle.close() }
                try handle.seekToEnd()
                try handle.write(contentsOf: data)
            }
            if call.getBool("last")!, rename(part.path, file.path) != 0 { throw CocoaError(.fileWriteUnknown) }
            return [:]
        }
    }

    /// Shows the file written in the system's preview, from where it can be shared to other apps too.
    @objc func openAttachment(_ call: CAPPluginCall) {
        work(call, "打开附件", on: files) {
            let file = try self.attachment(call)
            DispatchQueue.main.async {
                self.previewing = file
                let preview = QLPreviewController()
                preview.dataSource = self
                self.bridge?.viewController?.present(preview, animated: true)
            }
            return [:]
        }
    }

    /// Asks where to save the file written, and copies it there: `saved`, the file's name, or nothing
    /// when the user cancelled.
    @objc func saveAttachment(_ call: CAPPluginCall) {
        files.async {
            do {
                let file = try self.attachment(call)
                self.pick(UIDocumentPickerViewController(forExporting: [file], asCopy: true)) { saved in
                    call.resolve(saved.map { ["saved": $0.lastPathComponent] } ?? [:])
                }
            } catch {
                call.reject(forUser("保存附件", error))
            }
        }
    }

    /// Shows `picker`, made on the main thread as UIKit wants: `picked` hears what the user picked, nil
    /// when they cancelled.
    private func pick(_ picker: @autoclosure @escaping () -> UIDocumentPickerViewController, _ picked: @escaping (URL?) -> Void) {
        DispatchQueue.main.async {
            self.picked = picked
            let picker = picker()
            picker.delegate = self
            self.bridge?.viewController?.present(picker, animated: true)
        }
    }

    public func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        picked?(urls.first)
        picked = nil
    }

    public func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        picked?(nil)
        picked = nil
    }

    public func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

    public func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem { previewing! as NSURL }
}
