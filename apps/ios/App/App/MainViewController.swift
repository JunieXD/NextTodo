import Capacitor
import UIKit

/// The page, with the app's own plugin.
class MainViewController: CAPBridgeViewController {
    /// The one shown, to load again for local mode's new address. On the main thread.
    static weak var shown: MainViewController?

    override func capacitorDidLoad() {
        Self.shown = self
        bridge?.registerPluginInstance(NextTodoPlugin())
        Opening.loaded()
    }
}

/// What a notification opens in the page, `nexttodo://task/<id>` or `nexttodo://home`, as Capacitor's
/// `appUrlOpen`, which the page listens for; held until the bridge has loaded, should the tap have
/// started the app. On the main thread.
enum Opening {
    private static var loadedYet = false
    private static var pending: URL?

    static func open(_ url: URL) {
        DispatchQueue.main.async {
            if loadedYet { post(url) } else { pending = url }
        }
    }

    static func loaded() {
        loadedYet = true
        if let url = pending { post(url) }
        pending = nil
    }

    /// The App plugin keeps it until the page listens.
    private static func post(_ url: URL) {
        NotificationCenter.default.post(name: .capacitorOpenURL, object: ["url": url, "options": [:]])
    }
}
