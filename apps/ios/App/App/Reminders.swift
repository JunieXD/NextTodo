import UserNotifications

/// Deadline reminders (docs/ios.md#截止提醒): the list the page hands over, scheduled with the system,
/// which shows each when due, the app running or not. One thing at a time, on `queue`.
enum Reminders {
    static let queue = DispatchQueue(label: "cn.juniexd.nexttodo.reminders")

    private static let center = UNUserNotificationCenter.current()
    /// The list last handed over, as JSON.
    private static let LIST = "reminders.list"
    /// The reminders put off on this phone, by task, as JSON `{task: {at, until}}`.
    private static let SNOOZED = "reminders.snoozed"
    /// The most the system keeps scheduled for an app.
    private static let MOST = 64

    /// Categories of a reminder: of a task that can be marked done when it shows, or one that cannot.
    static let DONE = "remind"
    static let WAITING = "remind-waiting"

    /// How long a reminder can be put off by: the presets of Android's.
    static let SNOOZES = [(15, "15 分钟后提醒"), (60, "1 小时后提醒"), (180, "3 小时后提醒")]

    /// A reminder as `apps/web/src/mobile.ts` gives it: `at` in ms; `waiting`, how many of its
    /// prerequisites are not done.
    private struct Reminder: Codable {
        let task: String
        let at: Int64
        let title: String
        let body: String
        let waiting: Int
        let start: String?
        let start_time: String?
    }

    /// Put off: until when, the reminder at `at`.
    private struct Snooze: Codable {
        let at: Int64
        let until: Int64
    }

    private static var snoozed: [String: Snooze] {
        get { UserDefaults.standard.data(forKey: SNOOZED).map { try! JSONDecoder().decode([String: Snooze].self, from: $0) } ?? [:] }
        set { UserDefaults.standard.set(try! JSONEncoder().encode(newValue), forKey: SNOOZED) }
    }

    /// The actions on a reminder, as the app starts: before the system hands it one.
    static func register() {
        let later = SNOOZES.map { UNNotificationAction(identifier: "snooze-\($0.0)", title: $0.1, options: []) }
        center.setNotificationCategories([
            UNNotificationCategory(identifier: DONE, actions: [UNNotificationAction(identifier: "done", title: "完成", options: [])] + later, intentIdentifiers: []),
            UNNotificationCategory(identifier: WAITING, actions: later, intentIdentifiers: []),
        ])
    }

    /// Takes `json`, the reminders as the page reckons them, past ones too. The first time there are
    /// some, asks for notifications.
    static func set(_ json: String) throws {
        dispatchPrecondition(condition: .onQueue(queue))
        let list = try JSONDecoder().decode([Reminder].self, from: Data(json.utf8))
        if !list.isEmpty, settings().authorizationStatus == .notDetermined {
            _ = wait { done in center.requestAuthorization(options: [.alert, .sound, .badge]) { allowed, _ in done(allowed) } }
        }
        UserDefaults.standard.set(json, forKey: LIST)
        let byTask = Dictionary(list.map { ($0.task, $0) }) { first, _ in first }
        // Put off, a reminder stays so while its time is the same.
        snoozed = snoozed.filter { byTask[$0.key]?.at == $0.value.at }
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        center.removeAllPendingNotificationRequests()
        let due = list.map { ($0, snoozed[$0.task]?.until ?? $0.at) }.filter { $0.1 > now }.sorted { $0.1 < $1.1 }
        for (reminder, at) in due.prefix(MOST) { center.add(request(reminder, at: at, now: now)) }
        // Shown already: those no longer in the list, or of another time, go.
        let shown = wait { center.getDeliveredNotifications(completionHandler: $0) }.filter(isReminder)
        center.removeDeliveredNotifications(withIdentifiers: shown.filter { byTask[$0.request.identifier]?.at != at(of: $0.request.content) }.map(\.request.identifier))
    }

    /// Takes away every reminder, those shown already too.
    static func clear() {
        dispatchPrecondition(condition: .onQueue(queue))
        UserDefaults.standard.removeObject(forKey: LIST)
        UserDefaults.standard.removeObject(forKey: SNOOZED)
        center.removeAllPendingNotificationRequests()
        center.removeDeliveredNotifications(withIdentifiers: wait { center.getDeliveredNotifications(completionHandler: $0) }.filter(isReminder).map(\.request.identifier))
    }

    /// `task` was marked done from its reminder at `at`: shown or put off, that one goes. A repeating
    /// task's next, which the page may have handed over by the time the server answered, stays.
    static func completed(task: String, at: Int64) {
        dispatchPrecondition(condition: .onQueue(queue))
        if snoozed[task]?.at == at { snoozed[task] = nil }
        let pending = wait { center.getPendingNotificationRequests(completionHandler: $0) }
        if pending.contains(where: { $0.identifier == task && Reminders.at(of: $0.content) == at }) {
            center.removePendingNotificationRequests(withIdentifiers: [task])
        }
        let shown = wait { center.getDeliveredNotifications(completionHandler: $0) }
        if shown.contains(where: { $0.request.identifier == task && Reminders.at(of: $0.request.content) == at }) {
            center.removeDeliveredNotifications(withIdentifiers: [task])
        }
    }

    /// Shows the reminder of `content` again `minutes` from now, on this phone only.
    static func snooze(task: String, content: UNNotificationContent, minutes: Int) {
        dispatchPrecondition(condition: .onQueue(queue))
        snoozed[task] = Snooze(at: at(of: content), until: Int64(Date().timeIntervalSince1970 * 1000) + Int64(minutes) * 60_000)
        center.add(UNNotificationRequest(identifier: task, content: content, trigger: UNTimeIntervalNotificationTrigger(timeInterval: Double(minutes * 60), repeats: false)))
    }

    /// Says a change made here was turned down: which, and why. Tapping it opens the app.
    static func refused(id: String, what: String, message: String) {
        let content = UNMutableNotificationContent()
        content.title = "\(what)失败"
        content.body = message
        center.add(UNNotificationRequest(identifier: id, content: content, trigger: nil))
    }

    static func settings() -> UNNotificationSettings { wait { center.getNotificationSettings(completionHandler: $0) } }

    /// Asks for notifications, unless already asked: whether they are allowed.
    static func allow() -> Bool {
        if settings().authorizationStatus == .notDetermined {
            return wait { done in center.requestAuthorization(options: [.alert, .sound, .badge]) { allowed, _ in done(allowed) } }
        }
        return settings().authorizationStatus == .authorized
    }

    private static func request(_ r: Reminder, at: Int64, now: Int64) -> UNNotificationRequest {
        let content = UNMutableNotificationContent()
        content.title = r.title
        content.body = r.body
        content.sound = .default
        content.categoryIdentifier = completable(r, at: at) ? DONE : WAITING
        // The reminder's own time, which one put off keeps.
        content.userInfo = ["at": r.at]
        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: Double(at - now) / 1000, repeats: false)
        return UNNotificationRequest(identifier: r.task, content: content, trigger: trigger)
    }

    /// Whether `r`'s task can be marked done as it shows at `at`: it waits for nothing, and has started
    /// by then, by this phone's clock. Decided as it is scheduled: the app may not run when it shows.
    private static func completable(_ r: Reminder, at: Int64) -> Bool {
        guard r.waiting == 0 else { return false }
        guard let start = r.start else { return true }
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyy-MM-dd HH:mm"
        return Int64(format.date(from: "\(start) \(r.start_time ?? "00:00")")!.timeIntervalSince1970 * 1000) <= at
    }

    private static func isReminder(_ n: UNNotification) -> Bool { [DONE, WAITING].contains(n.request.content.categoryIdentifier) }

    /// The time of the reminder `content` shows, which one put off keeps.
    static func at(of content: UNNotificationContent) -> Int64 { (content.userInfo["at"] as! NSNumber).int64Value }

    /// Waits for what the system answers `ask` with: off the main thread.
    private static func wait<T>(_ ask: (@escaping (T) -> Void) -> Void) -> T {
        var answer: T!
        let done = DispatchSemaphore(value: 0)
        ask {
            answer = $0
            done.signal()
        }
        done.wait()
        return answer
    }
}

/// What the user does with a notification: opens the task, marks it done, or puts it off; set as the
/// app starts, so that one done in the background, the app not running, is heard.
final class NotificationActions: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationActions()

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler show: @escaping (UNNotificationPresentationOptions) -> Void) {
        show([.banner, .list, .sound])
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler done: @escaping () -> Void) {
        let request = response.notification.request
        let task = request.identifier
        guard [Reminders.DONE, Reminders.WAITING].contains(request.content.categoryIdentifier) else {
            Opening.open(URL(string: "nexttodo://home")!)
            return done()
        }
        switch response.actionIdentifier {
        case "done":
            Local.queue.async {
                // In local mode the server answers: started, or started again where it was should the
                // system have taken its port while the app was suspended. Should it not, the change is
                // kept for later.
                if Local.on {
                    if Local.running { _ = Local.resume() } else { _ = try? Local.open() }
                }
                Changes.queue.async {
                    if case let .refused(message) = Changes.complete(task: task, title: request.content.title, at: Reminders.at(of: request.content)) {
                        Reminders.queue.sync { Reminders.refused(id: Server.uuid7(), what: "完成“\(request.content.title)”", message: message) }
                    }
                    done()
                }
            }
        case let action where action.hasPrefix("snooze-"):
            Reminders.queue.async {
                Reminders.snooze(task: task, content: request.content, minutes: Int(action.dropFirst("snooze-".count))!)
                done()
            }
        case UNNotificationDefaultActionIdentifier:
            Opening.open(URL(string: "nexttodo://task/\(task)")!)
            done()
        default:
            done()
        }
    }
}
