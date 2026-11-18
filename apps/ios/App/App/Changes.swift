import Foundation

/// Changes made natively, without the app's page: tasks marked done on a reminder
/// (docs/ios.md#原生发出的改动). They go to the server in the order made; offline they are kept, and sent
/// with the next one or as the app comes to the front; the page takes them in too when it opens. One
/// thing at a time, on `queue`.
enum Changes {
    static let queue = DispatchQueue(label: "cn.juniexd.nexttodo.changes")

    /// The changes not sent yet, as JSON of `Kept`.
    private static let KEPT = "changes.kept"
    /// Who is signed in, as the page last said; none before it first has.
    private static let USER = "changes.user"

    enum Outcome {
        /// The server has it.
        case sent
        /// Kept, to send later: offline, or signed out meanwhile.
        case kept
        /// Turned down by the server; `message` is its reason, for the user.
        case refused(String)
    }

    /// A change kept to send: `task` done on `today`, the user's day there. `what` names it for the
    /// user, as in `完成“交房租”`, and `user` is who was signed in, once the page has said.
    private struct Change: Codable {
        let id: String
        let task: String
        let today: String
        let what: String
        var user: String?

        /// As the server and the page take it.
        var ops: [[String: Any]] { [["op": "task.update", "id": task, "done": true, "today": today]] }
    }

    /// What `KEPT` holds, oldest first: in a format of its own, which later versions read in. One not
    /// read, of another format, counts as none.
    private struct Kept: Codable {
        static let current = 1
        var format = current
        let changes: [Change]
    }

    private static var kept: [Change] {
        get {
            let read = UserDefaults.standard.data(forKey: KEPT).flatMap { try? JSONDecoder().decode(Kept.self, from: $0) }
            return read.flatMap { $0.format == Kept.current ? $0.changes : nil } ?? []
        }
        set { UserDefaults.standard.set(try! JSONEncoder().encode(Kept(changes: newValue)), forKey: KEPT) }
    }

    private static var user: String? { UserDefaults.standard.string(forKey: USER) }

    /// Marks `task`, titled `title`, done from its reminder at `at`; a repeating one moves on from today
    /// here. Done or kept, that reminder goes.
    static func complete(task: String, title: String, at: Int64) -> Outcome {
        dispatchPrecondition(condition: .onQueue(queue))
        let token = Server.token
        let day = DateFormatter()
        day.locale = Locale(identifier: "en_US_POSIX")
        day.dateFormat = "yyyy-MM-dd"
        let change = Change(id: Server.uuid7(), task: task, today: day.string(from: Date()), what: "完成“\(title)”", user: user)
        kept.append(change)
        let outcome = send(mine: change.id)
        if case .refused = outcome { return outcome }
        // Unless signed in again meanwhile, maybe as someone else, whose reminders these are.
        if token != nil, Server.token == token { Reminders.queue.sync { Reminders.completed(task: task, at: at) } }
        return outcome
    }

    /// Sends the changes kept, in order, until one cannot go: what became of `mine`. Without a session
    /// they wait for the next.
    @discardableResult
    static func send(mine: String? = nil) -> Outcome {
        dispatchPrecondition(condition: .onQueue(queue))
        var outcome = Outcome.kept
        // Both as they are now, for the whole batch: the page or local mode may change them meanwhile.
        guard let url = Prefs.string("server"), let token = Server.token else { return outcome }
        for change in kept {
            let id = change.id
            do {
                _ = try Server.mutate(url: url, token: token, id: id, ops: change.ops)
                kept.removeAll { $0.id == id }
                if id == mine { outcome = .sent }
            } catch let e as HttpError where e.status == 401 || e.status == 408 || e.status == 429 || e.status >= 500 {
                // Kept: for when the same user signs in again, as the page keeps its own; or for later,
                // past what may pass.
                return outcome
            } catch let e as HttpError {
                kept.removeAll { $0.id == id }
                if id == mine {
                    outcome = .refused(e.message)
                } else {
                    Reminders.queue.sync { Reminders.refused(id: id, what: change.what, message: e.message) }
                }
            } catch {
                return outcome
            }
        }
        return outcome
    }

    /// Signing in as `user`, before the session's token is written: the changes kept of anyone else
    /// go, so that none is sent under it. So do those from before the page first said who was signed
    /// in: whose they are is not known.
    static func signIn(user: String) {
        dispatchPrecondition(condition: .onQueue(queue))
        kept = kept.filter { $0.user == user }
        UserDefaults.standard.set(user, forKey: USER)
    }

    /// The page opened a session of `user`: the changes kept for them, as JSON `[{id, ops}]`, which it
    /// takes in, to show and send as its own. They stay kept here and are sent from here too, the
    /// server taking each once, so that none is lost should the app end before the page has saved
    /// them. Those of anyone else go; ones from before the page first said who is signed in were made in
    /// this session, signing in since having deleted them.
    static func takeOver(user: String) -> String {
        dispatchPrecondition(condition: .onQueue(queue))
        let theirs = kept.filter { ($0.user ?? user) == user }.map { change in
            var change = change
            change.user = user
            return change
        }
        kept = theirs
        UserDefaults.standard.set(user, forKey: USER)
        let changes: [[String: Any]] = theirs.map { ["id": $0.id, "ops": $0.ops] }
        return String(data: try! JSONSerialization.data(withJSONObject: changes), encoding: .utf8)!
    }

    /// Deletes the changes kept, and who is signed in: signed out, or the data is gone.
    static func forget() {
        dispatchPrecondition(condition: .onQueue(queue))
        UserDefaults.standard.removeObject(forKey: KEPT)
        UserDefaults.standard.removeObject(forKey: USER)
    }
}
