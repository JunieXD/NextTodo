import Foundation

/// A failure, with what to tell the user.
struct Failure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Local mode's server, `crates/ios`, called through `Local` one at a time. What fails throws with what
/// to tell the user.
enum LocalServer {
    /// Starts it on the data directory `dir`, at `port`, 0 for one the system picks; the same once
    /// started: `{url, user, epoch}`.
    static func start(dir: URL, port: UInt16) throws -> [String: Any] {
        let reply = answer(dir.path.withCString { nexttodo_start($0, port) })
        if let error = reply["error"] as? String { throw Local.StartFailed(message: error, locked: reply["locked"] as! Bool) }
        return reply["value"] as! [String: Any]
    }

    static func stop() { nexttodo_stop() }

    /// A new session's token, the earlier sessions revoked.
    static func session() throws -> String { try value(nexttodo_session()) as! String }

    /// Also off `Local.queue`: stopping the server meanwhile ends the wait.
    static func pull(waitMs: UInt64) { nexttodo_pull(waitMs) }

    /// Whether it is still the one at its address, not another program given its port meanwhile.
    static func answers() -> Bool { nexttodo_answers() }

    static func backup(to: URL) throws { _ = try value(to.path.withCString(nexttodo_backup)) }

    static func prepare(dir: URL, from: URL) throws {
        _ = try value(dir.path.withCString { dir in from.path.withCString { nexttodo_prepare(dir, $0) } })
    }

    static func commit(dir: URL) throws { _ = try value(dir.path.withCString(nexttodo_commit)) }

    /// What an attachment called `name` is called as a file (docs/desktop.md#附件): by the rule the other
    /// apps follow too.
    static func fileName(_ name: String) throws -> String { try value(name.withCString(nexttodo_file_name)) as! String }

    private static func answer(_ s: UnsafeMutablePointer<CChar>!) -> [String: Any] {
        defer { nexttodo_free(s) }
        return try! JSONSerialization.jsonObject(with: Data(String(cString: s).utf8)) as! [String: Any]
    }

    private static func value(_ s: UnsafeMutablePointer<CChar>!) throws -> Any {
        let reply = answer(s)
        if let error = reply["error"] as? String { throw Failure(message: error) }
        return reply["value"]!
    }
}
