import UserNotifications

/// A time of a repeating task, as `Occurrence` in `apps/web/src/api/types`: the one due on `due` after
/// it was done `completions` times (docs/api.md#改动).
struct Occurrence: Codable, Equatable {
    let due: String
    let completions: Int

    /// As JSON has it, in a change or a notification.
    var json: [String: Any] { ["due": due, "completions": completions] }
}

extension Optional where Wrapped == Occurrence {
    /// Whether a time marked `self`, every time of the task without one, is time `o`.
    func covers(_ o: Occurrence?) -> Bool { self == nil || self == o }
}

/// Deadline reminders (docs/ios.md#截止提醒): the list the page hands over, scheduled with the system,
/// which shows each when due, the app running or not. One thing at a time, on `queue`.
enum Reminders {
    static let queue = DispatchQueue(label: "cn.juniexd.nexttodo.reminders")

    private static let center = UNUserNotificationCenter.current()
    /// The list last handed over, as JSON.
    private static let LIST = "reminders.list"
    /// The reminders put off on this phone, by task, as JSON `{task: {at, until}}`: one a task, which the
    /// next of its reminders to come takes the place of.
    private static let SNOOZED = "reminders.snoozed"
    /// The daily digests last handed over, less the tasks done here since, as JSON.
    private static let DIGESTS = "reminders.digests"
    /// The times of tasks done here, as JSON of `[Done]`. A list handed over behind one leaves it out.
    private static let DONE_HERE = "reminders.done"
    /// The most the system keeps scheduled for an app.
    private static let MOST = 64

    /// Categories of a reminder: of a task that can be marked done when it shows, or one that cannot.
    static let DONE = "remind"
    static let WAITING = "remind-waiting"
    /// Of a daily digest, which has no actions: tapping it opens 接下来.
    private static let DIGEST = "digest"

    /// How long a reminder can be put off by: the presets of Android's.
    static let SNOOZES = [(15, "15 分钟后提醒"), (60, "1 小时后提醒"), (180, "3 小时后提醒")]

    /// A reminder as `apps/web/src/mobile.ts` gives it: `at` in ms; `occurrence`, the time it is of
    /// should the task repeat, the same for a task's reminders of one time; `waiting`, how many of its
    /// prerequisites are not done.
    private struct Reminder: Codable {
        let task: String
        let at: Int64
        let occurrence: Occurrence?
        let title: String
        let body: String
        let waiting: Int
        let start: String?
        let start_time: String?
    }

    /// The daily digests as `apps/web/src/mobile.ts` gives them, and whether they hide what they are of.
    private struct Digests: Codable {
        var hide: Bool
        var list: [Digest]
    }

    /// A daily digest: at `at`, in ms, of `tasks`, each with its time, as a reminder has it.
    private struct Digest: Codable {
        struct Left: Codable {
            let task: String
            let occurrence: Occurrence?
            let title: String
        }

        let at: Int64
        var tasks: [Left]
    }

    /// `task`'s time `occurrence`, every time of it without one, done here by change `id`, which became
    /// change `seq` on the server; none while not sent (docs/android.md#截止提醒).
    private struct Done: Codable {
        let id: String
        let task: String
        let occurrence: Occurrence?
        var seq: Int64?
    }

    private static var doneHere: [Done] {
        get { UserDefaults.standard.data(forKey: DONE_HERE).map { try! JSONDecoder().decode([Done].self, from: $0) } ?? [] }
        set { UserDefaults.standard.set(try! JSONEncoder().encode(newValue), forKey: DONE_HERE) }
    }

    /// `task`'s time `occurrence` is being done here, by change `id`.
    static func completing(id: String, task: String, occurrence: Occurrence?) {
        dispatchPrecondition(condition: .onQueue(queue))
        doneHere.append(Done(id: id, task: task, occurrence: occurrence))
    }

    /// Change `id` became change `seq` on the server.
    static func sent(id: String, seq: Int64) {
        dispatchPrecondition(condition: .onQueue(queue))
        doneHere = doneHere.map { d in
            var d = d
            if d.id == id { d.seq = seq }
            return d
        }
    }

    /// Change `id` was turned down: its time is to be done still.
    static func unmark(id: String) {
        dispatchPrecondition(condition: .onQueue(queue))
        doneHere.removeAll { $0.id == id }
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

    /// Takes `json`, the reminders as the page reckons them, past ones too, and `digest`, the daily
    /// digests, of its copy as of change `seq`. The times done here that the copy may not have yet, sent
    /// after `seq` or not sent, are left out. The first time there are some, asks for notifications.
    static func set(_ json: String, digest: String, seq: Int64) throws {
        dispatchPrecondition(condition: .onQueue(queue))
        let ahead = doneHere.filter { $0.seq.map { $0 > seq } ?? true }
        doneHere = ahead
        let done = { (task: String, o: Occurrence?) in ahead.contains { $0.task == task && $0.occurrence.covers(o) } }
        let list = try JSONDecoder().decode([Reminder].self, from: Data(json.utf8)).filter { !done($0.task, $0.occurrence) }
        var digests = try JSONDecoder().decode(Digests.self, from: Data(digest.utf8))
        digests.list = digests.list.map { d in
            var d = d
            d.tasks.removeAll { done($0.task, $0.occurrence) }
            return d
        }.filter { !$0.tasks.isEmpty }
        if !list.isEmpty || !digests.list.isEmpty, settings().authorizationStatus == .notDetermined {
            _ = wait { done in center.requestAuthorization(options: [.alert, .sound, .badge]) { allowed, _ in done(allowed) } }
        }
        stored = list
        storedDigests = digests
        schedule()
        // Shown already: those no longer in the list go, and those that say otherwise than they would
        // now, the content hidden since say, or are of a time done since. The system can't change one
        // shown.
        let wanted = Dictionary(list.map { (id(task: $0.task, at: $0.at), $0) }) { first, _ in first }
        let shown = wait { center.getDeliveredNotifications(completionHandler: $0) }.filter { isReminder($0.request.content) }
        let stale = shown.filter { n in
            let c = n.request.content
            return wanted[n.request.identifier].map { $0.title != c.title || $0.body != c.body || !occurrence(of: c).covers($0.occurrence) } ?? true
        }
        center.removeDeliveredNotifications(withIdentifiers: stale.map(\.request.identifier))
        // Hidden since one was shown, a digest may list titles still.
        if digests.hide {
            let listing = delivered(DIGEST).filter { !$0.request.content.body.isEmpty }
            center.removeDeliveredNotifications(withIdentifiers: listing.map(\.request.identifier))
        }
    }

    /// The digests last handed over, less the tasks done here since.
    private static var storedDigests: Digests {
        get { UserDefaults.standard.data(forKey: DIGESTS).map { try! JSONDecoder().decode(Digests.self, from: $0) } ?? Digests(hide: false, list: []) }
        set { UserDefaults.standard.set(try! JSONEncoder().encode(newValue), forKey: DIGESTS) }
    }

    /// Those shown of category `category`.
    private static func delivered(_ category: String) -> [UNNotification] {
        wait { center.getDeliveredNotifications(completionHandler: $0) }.filter { $0.request.content.categoryIdentifier == category }
    }

    /// Whether `task`'s time `occurrence`, any without one, is still to be done: neither done since from
    /// one of its reminders, nor done or deleted elsewhere since the last list.
    static func listed(task: String, occurrence: Occurrence?) -> Bool {
        dispatchPrecondition(condition: .onQueue(queue))
        return stored.contains { $0.task == task && occurrence.covers($0.occurrence) }
    }

    /// The list last handed over, less the times done since from a reminder.
    private static var stored: [Reminder] {
        get { UserDefaults.standard.string(forKey: LIST).map { try! JSONDecoder().decode([Reminder].self, from: Data($0.utf8)) } ?? [] }
        set { UserDefaults.standard.set(String(decoding: try! JSONEncoder().encode(newValue), as: UTF8.self), forKey: LIST) }
    }

    /// Has the system show the stored reminders and daily digests to come, the nearest `MOST`; a
    /// reminder put off, when it was put off to. That holds while the reminder is there at the same
    /// time, and until another of its task's comes first, as on Android: put off past the next, it would
    /// come twice.
    private static func schedule() {
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        let list = stored
        let put = snoozed.filter { task, s in
            list.contains { $0.task == task && $0.at == s.at } && !list.contains { $0.task == task && $0.at > now && $0.at <= s.until }
        }
        snoozed = put
        center.removeAllPendingNotificationRequests()
        let digests = storedDigests
        // A request only for what is to come: the system refuses a time past.
        let reminders = list.map { r in
            let at = put[r.task].flatMap { $0.at == r.at ? $0.until : nil } ?? r.at
            return (at, { request(r, at: at, now: now) })
        }
        let due: [(Int64, () -> UNNotificationRequest)] = reminders + digests.list.map { d in (d.at, { request(d, hide: digests.hide, now: now) }) }
        for (_, request) in due.filter({ $0.0 > now }).sorted(by: { $0.0 < $1.0 }).prefix(MOST) { center.add(request()) }
    }

    /// Takes away every reminder and daily digest, those shown already too.
    static func clear() {
        dispatchPrecondition(condition: .onQueue(queue))
        UserDefaults.standard.removeObject(forKey: LIST)
        UserDefaults.standard.removeObject(forKey: DIGESTS)
        UserDefaults.standard.removeObject(forKey: SNOOZED)
        UserDefaults.standard.removeObject(forKey: DONE_HERE)
        center.removeAllPendingNotificationRequests()
        let shown = wait { center.getDeliveredNotifications(completionHandler: $0) }.filter { isReminder($0.request.content) || $0.request.content.categoryIdentifier == DIGEST }
        center.removeDeliveredNotifications(withIdentifiers: shown.map(\.request.identifier))
    }

    /// `task` was marked done, its time `occurrence`, every time of it without one: shown or put off,
    /// those reminders go. A repeating task's next time's, which the page may have handed over by the
    /// time the server answered, stay.
    static func completed(task: String, occurrence: Occurrence?) {
        dispatchPrecondition(condition: .onQueue(queue))
        stored = stored.filter { $0.task != task || !occurrence.covers($0.occurrence) }
        var digests = storedDigests
        digests.list = digests.list.map { d in
            var d = d
            d.tasks.removeAll { $0.task == task && occurrence.covers($0.occurrence) }
            return d
        }.filter { !$0.tasks.isEmpty }
        storedDigests = digests
        schedule()
        let shown = wait { center.getDeliveredNotifications(completionHandler: $0) }
            .filter { isReminder($0.request.content) && Reminders.task(of: $0.request.content) == task && occurrence.covers(Reminders.occurrence(of: $0.request.content)) }
        center.removeDeliveredNotifications(withIdentifiers: shown.map(\.request.identifier))
    }

    /// Shows the reminder of `content` again `minutes` from now, on this phone only, as it is now: its
    /// content hidden since, say.
    static func snooze(content: UNNotificationContent, minutes: Int) {
        dispatchPrecondition(condition: .onQueue(queue))
        snoozed[task(of: content)] = Snooze(at: at(of: content), until: Int64(Date().timeIntervalSince1970 * 1000) + Int64(minutes) * 60_000)
        schedule()
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
        // A task's together.
        content.threadIdentifier = r.task
        // The reminder's own time, which one put off keeps.
        content.userInfo = ["task": r.task, "at": r.at]
        content.userInfo["occurrence"] = r.occurrence?.json
        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: Double(at - now) / 1000, repeats: false)
        return UNNotificationRequest(identifier: id(task: r.task, at: r.at), content: content, trigger: trigger)
    }

    /// `d`, as `digestText` in apps/web/src/domain/reminders.ts says it; `hide`, without the titles.
    private static func request(_ d: Digest, hide: Bool, now: Int64) -> UNNotificationRequest {
        let content = UNMutableNotificationContent()
        content.title = "今天还有 \(d.tasks.count) 项没做完"
        content.body = hide ? "" : d.tasks.map(\.title).joined(separator: "、")
        content.sound = .default
        content.categoryIdentifier = DIGEST
        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: Double(d.at - now) / 1000, repeats: false)
        return UNNotificationRequest(identifier: "\(DIGEST) \(d.at)", content: content, trigger: trigger)
    }

    /// A reminder's notification: one for each of a task's reminders, which the system can't have take
    /// another's place once shown.
    private static func id(task: String, at: Int64) -> String { "\(task) \(at)" }

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

    private static func isReminder(_ content: UNNotificationContent) -> Bool { [DONE, WAITING].contains(content.categoryIdentifier) }

    /// The time of the reminder `content` shows, which one put off keeps.
    static func at(of content: UNNotificationContent) -> Int64 { (content.userInfo["at"] as! NSNumber).int64Value }

    /// The time the reminder `content` shows is of; none of a task that does not repeat, nor shown by an
    /// earlier version.
    static func occurrence(of content: UNNotificationContent) -> Occurrence? {
        (content.userInfo["occurrence"] as? [String: Any]).map { Occurrence(due: $0["due"] as! String, completions: ($0["completions"] as! NSNumber).intValue) }
    }

    /// The task the reminder `content` shows is of.
    static func task(of content: UNNotificationContent) -> String { content.userInfo["task"] as! String }

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
/// app starts, so that one done in the background, the app not running, is heard. UIKit takes `done`
/// on the main thread only: called on another, it ends the app.
final class NotificationActions: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationActions()

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler show: @escaping (UNNotificationPresentationOptions) -> Void) {
        show([.banner, .list, .sound])
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler done: @escaping () -> Void) {
        let request = response.notification.request
        guard [Reminders.DONE, Reminders.WAITING].contains(request.content.categoryIdentifier) else {
            Opening.open(URL(string: "nexttodo://home")!)
            return done()
        }
        let task = Reminders.task(of: request.content)
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
                    if case let .refused(message)? = Changes.complete(task: task, title: request.content.title, occurrence: Reminders.occurrence(of: request.content)) {
                        Reminders.queue.sync { Reminders.refused(id: Server.uuid7(), what: "完成“\(request.content.title)”", message: message) }
                    }
                    DispatchQueue.main.async(execute: done)
                }
            }
        case let action where action.hasPrefix("snooze-"):
            Reminders.queue.async {
                Reminders.snooze(content: request.content, minutes: Int(action.dropFirst("snooze-".count))!)
                DispatchQueue.main.async(execute: done)
            }
        case UNNotificationDefaultActionIdentifier:
            Opening.open(URL(string: "nexttodo://task/\(task)")!)
            done()
        default:
            done()
        }
    }
}
