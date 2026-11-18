import Foundation

/// Local mode (docs/ios.md#本机模式): the server in the app's process, started when the page or a
/// reminder needs it. iOS apps cannot start afresh: to enter or leave local mode, or to replace the
/// data, the server stops and the page loads again. One thing at a time, on `queue`.
enum Local {
    static let queue = DispatchQueue(label: "cn.juniexd.nexttodo.local")

    /// Capacitor's `mode`: `local` while the app is in local mode.
    private static let MODE = "mode"
    /// The epoch of the database the reminders are of (docs/local.md#数据代号).
    private static let EPOCH = "localEpoch"

    /// The server did not start; `locked` while it still lets go of its data, having just stopped.
    struct StartFailed: LocalizedError {
        let message: String
        let locked: Bool
        var errorDescription: String? { message }
    }

    static var on: Bool { Prefs.string(MODE) == "local" }

    static let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("local")

    /// What the page needs, once the server has started and the app signed in to it.
    private static var opened: [String: Any]?

    /// Whether the server has started and the app signed in to it, for the page or a reminder's action.
    static var running: Bool {
        dispatchPrecondition(condition: .onQueue(queue))
        return opened != nil
    }

    /// Starts the server and signs the app in: its address, the session's token, the local user and
    /// the epoch. Once done, the same each time, until the server stops.
    static func open() throws -> [String: Any] {
        dispatchPrecondition(condition: .onQueue(queue))
        if let opened { return opened }
        // The last run's, at a port no longer this app's: should the server not start, nothing goes there.
        Prefs.remove("server")
        Prefs.remove("token")
        var started = try start()
        let epoch = started["epoch"] as! String
        if Prefs.string(EPOCH) != epoch {
            // The reminders and the changes not sent are of data that is not there any more.
            Reminders.queue.sync { Reminders.clear() }
            Changes.queue.sync { Changes.forget() }
            Prefs.set(EPOCH, epoch)
        }
        started["token"] = try LocalServer.session()
        Prefs.set("server", started["url"] as! String)
        Prefs.set("token", started["token"] as! String)
        opened = started
        return started
    }

    /// Starts the server, the first time since it last stopped: its address, the local user and the epoch.
    static func start() throws -> [String: Any] {
        dispatchPrecondition(condition: .onQueue(queue))
        return try LocalServer.start(dir: dir, port: 0)
    }

    /// Stops the server, for the page to load again.
    static func stop() {
        dispatchPrecondition(condition: .onQueue(queue))
        LocalServer.stop()
        opened = nil
    }

    /// Enters local mode, from the page's next load; or leaves it, and the session with it.
    static func set(on: Bool) {
        if on {
            Prefs.set(MODE, "local")
        } else {
            for key in [MODE, "server", "token", EPOCH] { Prefs.remove(key) }
        }
    }

    /// Back in the front, or woken in the background for a reminder's action: the system may have
    /// taken the server's port while the app was suspended. It starts again at the same one, where the
    /// page goes on; when it cannot, the page loads again, for a new address, and false. Then it looks
    /// at the drive now (docs/local.md#运行).
    static func resume() -> Bool {
        dispatchPrecondition(condition: .onQueue(queue))
        let url = opened!["url"] as! String
        if !Server.answers(url) {
            LocalServer.stop()
            do {
                _ = try LocalServer.start(dir: dir, port: UInt16(URLComponents(string: url)!.port!))
            } catch {
                // At a port no longer this app's: changes made natively are kept until the page has a new one.
                opened = nil
                Prefs.remove("server")
                Prefs.remove("token")
                DispatchQueue.main.async { MainViewController.shown?.loadWebView() }
                return false
            }
        }
        LocalServer.pull(waitMs: 0)
        return true
    }

    static func backup(to: URL) throws {
        dispatchPrecondition(condition: .onQueue(queue))
        try LocalServer.backup(to: to)
    }

    /// Prepares the backup at `from` to replace the data; without the server, which may not have started.
    static func prepare(from: URL) throws {
        dispatchPrecondition(condition: .onQueue(queue))
        try LocalServer.prepare(dir: dir, from: from)
    }

    /// The user's go-ahead: the backup prepared replaces the data as the server starts next.
    static func commit() throws {
        dispatchPrecondition(condition: .onQueue(queue))
        try LocalServer.commit(dir: dir)
    }
}
