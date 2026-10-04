import Cocoa
import Sparkle

final class OnGrowUpdater: NSObject, SPUUpdaterDelegate {
    private var controller: SPUStandardUpdaterController?
    private var freshAppcastPermitted = false
    // All entry points, including cached update resumes, stay disabled until
    // the durable production guardian/session transaction is connected.
    private let sessionBarrierReady = false

    func startIfSafe() {
        guard mayStart() else { return }
        controller = SPUStandardUpdaterController(startingUpdater: false,
            updaterDelegate: self, userDriverDelegate: nil)
        controller?.startUpdater()
    }

    private func mayStart() -> Bool {
        let bundle = Bundle.main
        let path = bundle.bundleURL.standardizedFileURL.path
        return OnGrowUpdatePolicy.mayStart(bundle.infoDictionary ?? [:],
            environment: ProcessInfo.processInfo.environment,
            installed: path.hasPrefix("/Applications/") && path.hasSuffix(".app"),
            translocated: path.contains("/AppTranslocation/"),
            sessionBarrierReady: sessionBarrierReady)
    }

    @objc(updater:mayPerformUpdateCheck:error:)
    func updater(_ updater: SPUUpdater, mayPerform updateCheck: SPUUpdateCheck) throws {
        freshAppcastPermitted = false
        guard mayStart() else { throw rejectedUpdate() }
    }

    @objc(updater:didFinishLoadingAppcast:)
    func updater(_ updater: SPUUpdater, didFinishLoading appcast: SUAppcast) {
        // Sparkle can select a delta with the full archive as its fallback.
        // Validate every initial candidate before the SDK makes that choice.
        freshAppcastPermitted = appcast.signingValidationStatus == .succeeded &&
            !appcast.items.isEmpty && appcast.items.count <= 256 &&
            appcast.items.allSatisfy { permitted($0) }
    }

    @objc(updater:shouldProceedWithUpdate:updateCheck:error:)
    func updater(_ updater: SPUUpdater, shouldProceedWithUpdate item: SUAppcastItem,
                 updateCheck: SPUUpdateCheck) throws {
        guard mayStart(), freshAppcastPermitted, permitted(item) else { throw rejectedUpdate() }
    }

    private func permitted(_ item: SUAppcastItem) -> Bool {
        let deltas = item.deltaUpdates?.values.map { $0.fileURL?.absoluteString } ?? []
        return OnGrowUpdatePolicy.mayProceed(Bundle.main.infoDictionary ?? [:],
            feedSigned: item.signingValidationStatus == .succeeded,
            archive: item.fileURL?.absoluteString, deltaArchives: deltas,
            installationType: item.installationType)
    }

    @objc(updater:shouldDownloadReleaseNotesForUpdate:)
    func updater(_ updater: SPUUpdater, shouldDownloadReleaseNotesForUpdate item: SUAppcastItem) -> Bool {
        return false
    }

    private func rejectedUpdate() -> NSError {
        return NSError(domain: "de.ongrow.update", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "Das Update konnte nicht geprüft werden.",
            NSLocalizedRecoverySuggestionErrorKey: "Bitte wende dich an den OnGROW Support."
        ])
    }

    @objc(feedURLStringForUpdater:)
    func feedURLString(for updater: SPUUpdater) -> String? {
        // Never obtain a feed URL from mutable user defaults.
        return Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String
    }
}
