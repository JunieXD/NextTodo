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
        Local.queue.async {
            if Local.on {
                // Not before it has started: the address kept until then is the last run's.
                guard Local.running, Local.resume() else { return }
            }
            Changes.queue.async { Changes.send() }
        }
    }

    /// Changes the app's server has go on into the drive: the app asks for the time it takes, which the
    /// system gives for a while (docs/local/drive.md#运行). Waited for off `Local.queue`, so that coming
    /// back to the front does not wait for the drive.
    func sceneDidEnterBackground(_ scene: UIScene) {
        var task = UIBackgroundTaskIdentifier.invalid
        let end = {
            guard task != .invalid else { return }
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
        }
        task = UIApplication.shared.beginBackgroundTask(withName: "drive", expirationHandler: end)
        guard task != .invalid else { return }
        Local.queue.async {
            guard Local.on, Local.running else { return DispatchQueue.main.async(execute: end) }
            DispatchQueue.global().async {
                LocalServer.pull(waitMs: 15_000)
                DispatchQueue.main.async(execute: end)
            }
        }
    }
}
