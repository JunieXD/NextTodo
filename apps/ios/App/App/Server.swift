import Foundation

/// Capacitor Preferences, where the page keeps the server's address and the session's token: each key
/// is `CapacitorStorage.<key>` in the app's `UserDefaults`.
enum Prefs {
    static func string(_ key: String) -> String? { UserDefaults.standard.string(forKey: "CapacitorStorage." + key) }
    static func set(_ key: String, _ value: String) { UserDefaults.standard.set(value, forKey: "CapacitorStorage." + key) }
    static func remove(_ key: String) { UserDefaults.standard.removeObject(forKey: "CapacitorStorage." + key) }
}

/// A request the server answered with an error, with its message.
struct HttpError: LocalizedError {
    let status: Int
    let message: String
    var errorDescription: String? { message }
}

/// The server, reached with the address and session token the page keeps in Capacitor Preferences.
enum Server {
    /// The session's token; nil when signed out.
    static var token: String? { Prefs.string("token") }

    /// Submits the change `id`, made of `ops`, to the server at `url`; one submitted before gives what it
    /// did then. Returns the change's sequence number. Throws `HttpError` when the server turns it down,
    /// `URLError` when it cannot be reached.
    static func mutate(url: String, token: String, id: String, ops: Any) throws -> Int64 {
        var request = URLRequest(url: URL(string: url + "/api/v1/mutations")!, timeoutInterval: 10)
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["id": id, "ops": ops])
        let (data, response) = try send(request)
        guard (200..<300).contains(response.statusCode) else {
            let message = (try? JSONDecoder().decode(Refusal.self, from: data))?.message
            throw HttpError(status: response.statusCode, message: message ?? "服务器错误（\(response.statusCode)），请稍后重试")
        }
        // Not as the server answers, as from a proxy in between: kept, to send again.
        return try JSONDecoder().decode(Applied.self, from: data).seq
    }

    /// What the server answers a change with, as far as it is used.
    private struct Applied: Decodable {
        let seq: Int64
    }

    /// What the server says to turn something down.
    private struct Refusal: Decodable {
        let message: String
    }

    /// The app's own server, on this device, is reached directly: the system's proxy would get the token
    /// (docs/local.md#页面直连). Told by the address, not the mode: a batch of changes keeps the address it
    /// began with.
    private static let direct: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.connectionProxyDictionary = [:]
        return URLSession(configuration: config)
    }()

    /// Sends `request` and waits for the answer: called off the main thread, one thing at a time.
    private static func send(_ request: URLRequest) throws -> (Data, HTTPURLResponse) {
        var result: Result<(Data, HTTPURLResponse), Error>!
        let done = DispatchSemaphore(value: 0)
        (request.url!.host == "127.0.0.1" ? direct : URLSession.shared).dataTask(with: request) { data, response, error in
            result = error.map { .failure($0) } ?? .success((data!, response as! HTTPURLResponse))
            done.signal()
        }.resume()
        done.wait()
        return try result.get()
    }

    /// Ids of changes are UUIDv7: the time in milliseconds, then random bits.
    static func uuid7() -> String {
        var bytes = (0..<16).map { _ in UInt8.random(in: 0...255) }
        let ms = UInt64(Date().timeIntervalSince1970 * 1000)
        for i in 0..<6 { bytes[i] = UInt8(truncatingIfNeeded: ms >> (40 - 8 * UInt64(i))) }
        bytes[6] = bytes[6] & 0x0F | 0x70
        bytes[8] = bytes[8] & 0x3F | 0x80
        return NSUUID(uuidBytes: bytes).uuidString.lowercased()
    }
}
