import Foundation

/// Changes made natively, without the app's page: tasks marked done on a reminder
/// (docs/ios.md#原生发出的改动). They go to the server in the order made; offline they are kept, and sent
/// with the next one or as the app comes to the front; the page takes them in too when it opens. One
/// thing at a time, on `queue`.
enum Changes {
    static let queue = DispatchQueue(label: "cn.juniexd.nexttodo.changes")

    /// The changes not sent yet, oldest first, as JSON: `{id, ops, what, user}`, `what` naming it for
    /// the user, as in `完成“交房租”`, and `user` who was signed in.
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

    private static var kept: [[String: Any]] {
        get { UserDefaults.standard.data(forKey: KEPT).map { try! JSONSerialization.jsonObject(with: $0) as! [[String: Any]] } ?? [] }
        set { UserDefaults.standard.set(try! JSONSerialization.data(withJSONObject: newValue), forKey: KEPT) }
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
        let op: [String: Any] = ["op": "task.update", "id": task, "done": true, "today": day.string(from: Date())]
        var change: [String: Any] = ["id": Server.uuid7(), "ops": [op], "what": "完成“\(title)”"]
        change["user"] = user
        kept.append(change)
        let outcome = send(mine: change["id"] as? String)
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
            let id = change["id"] as! String
            do {
                _ = try Server.mutate(url: url, token: token, id: id, ops: change["ops"]!)
                kept.removeAll { $0["id"] as? String == id }
                if id == mine { outcome = .sent }
            } catch let e as HttpError where e.status == 401 || e.status >= 500 {
                // Kept: for when the same user signs in again, as the page keeps its own; or for later.
                return outcome
            } catch let e as HttpError {
                kept.removeAll { $0["id"] as? String == id }
                if id == mine {
                    outcome = .refused(e.message)
                } else {
                    Reminders.queue.sync { Reminders.refused(id: id, what: change["what"] as! String, message: e.message) }
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
        kept = kept.filter { $0["user"] as? String == user }
        UserDefaults.standard.set(user, forKey: USER)
    }

    /// The page opened a session of `user`: the changes kept for them, as JSON `[{id, ops}]`, which it
    /// takes in, to show and send as its own. They stay kept here and are sent from here too, the
    /// server taking each once, so that none is lost should the app end before the page has saved
    /// them. Those of anyone else go; ones from before the page first said who is signed in were made in
    /// this session, signing in since having deleted them.
    static func takeOver(user: String) -> String {
        dispatchPrecondition(condition: .onQueue(queue))
        let theirs = kept.filter { ($0["user"] as? String ?? user) == user }.map { $0.merging(["user": user]) { $1 } }
        kept = theirs
        UserDefaults.standard.set(user, forKey: USER)
        let changes = theirs.map { ["id": $0["id"]!, "ops": $0["ops"]!] }
        return String(data: try! JSONSerialization.data(withJSONObject: changes), encoding: .utf8)!
    }

    /// Deletes the changes kept, and who is signed in: signed out, or the data is gone.
    static func forget() {
        dispatchPrecondition(condition: .onQueue(queue))
        UserDefaults.standard.removeObject(forKey: KEPT)
        UserDefaults.standard.removeObject(forKey: USER)
    }
}
