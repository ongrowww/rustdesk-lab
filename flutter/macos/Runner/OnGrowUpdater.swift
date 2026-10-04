import Cocoa
import Sparkle

final class OnGrowUpdater: NSObject, SPUUpdaterDelegate {
    private var controller: SPUStandardUpdaterController?

    func startIfSafe() {
        let bundle = Bundle.main
        let path = bundle.bundleURL.standardizedFileURL.path
        // A relaunch delegate is not an installation barrier. Until the Rust
        // service/session transaction exists, never start any update cycle.
        let sessionBarrierReady = false
        guard OnGrowUpdatePolicy.mayStart(bundle.infoDictionary ?? [:],
            environment: ProcessInfo.processInfo.environment,
            installed: path.hasPrefix("/Applications/") && path.hasSuffix(".app"),
            translocated: path.contains("/AppTranslocation/"),
            sessionBarrierReady: sessionBarrierReady) else { return }
        controller = SPUStandardUpdaterController(startingUpdater: false,
            updaterDelegate: self, userDriverDelegate: nil)
        controller?.startUpdater()
    }

    func feedURLString(for updater: SPUUpdater) -> String? {
        // Never obtain a feed URL from mutable user defaults.
        return Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String
    }
}
