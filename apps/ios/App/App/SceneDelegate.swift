import Capacitor
import UIKit

class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let windowScene = scene as? UIWindowScene else { return }

        window = UIWindow(windowScene: windowScene)
        window?.rootViewController = MainViewController()
        window?.makeKeyAndVisible()

        SceneDelegateProxy.shared.scene(scene, willConnectTo: session, options: connectionOptions)
    }

    func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) {
        SceneDelegateProxy.shared.scene(scene, openURLContexts: URLContexts)
    }

    func scene(_ scene: UIScene, continue userActivity: NSUserActivity) {
        SceneDelegateProxy.shared.scene(scene, continue: userActivity)
    }

    /// The local server goes on where it was, and changes made on reminders meanwhile go out.
    func sceneWillEnterForeground(_ scene: UIScene) {
        let page = window?.rootViewController as? CAPBridgeViewController
        Local.queue.async {
            if Local.on {
                // Not before the page has opened it: the address kept until then is the last run's.
                guard Local.running else { return }
                guard Local.resume() else { return DispatchQueue.main.async { page?.loadWebView() } }
            }
            Changes.queue.async { Changes.send() }
        }
    }
}
