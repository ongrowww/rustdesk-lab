import Foundation
#if ONGROW_SPARKLE_DELEGATE_PROBE
import Sparkle
#endif

@main
struct PolicyTests {
    @MainActor static func main() {
        var count = 0
        for role in OnGrowUpdatePolicy.products.keys {
            let product = OnGrowUpdatePolicy.products[role]!
            var info: [String: Any] = ["OnGrowUpdateRole": role,
                "CFBundleIdentifier": product.0, "CFBundleDisplayName": product.1,
                "SUFeedURL": "https://ongrow.de/assets/updates/\(role)/macos-arm64/appcast.xml",
                "SUPublicEDKey": Data(repeating: 1, count: 32).base64EncodedString(),
                "OnGrowUpdateSequence": "12", "CFBundleVersion": "1.0.12", "SUVerifyUpdateBeforeExtraction": true,
                "SURequireSignedFeed": true, "SUSignedFeedFailureExpirationInterval": 0,
                "SUEnableSystemProfiling": false, "OnGrowUpdateApplyEnabled": false]
            precondition(OnGrowUpdatePolicy.configured(info)); count += 1
            let archive = "https://ongrow.de/assets/updates/\(role)/macos-arm64/update-1.0.13.zip"
            let delta = "https://ongrow.de/assets/updates/\(role)/macos-arm64/update-1.0.12-1.0.13.delta"
            func proceed(_ signed: Bool = true, _ url: String? = archive,
                         _ deltas: [String?] = [], _ kind: String = "application") -> Bool {
                OnGrowUpdatePolicy.mayProceed(info, feedSigned: signed, archive: url,
                    deltaArchives: deltas, installationType: kind)
            }
            precondition(proceed()); count += 1
            precondition(proceed(true, archive, [delta])); count += 1
            precondition(!proceed(false)); count += 1
            precondition(!proceed(true, nil)); count += 1
            precondition(!proceed(true, archive, [nil])); count += 1
            precondition(!proceed(true, archive, [], "package")); count += 1
            precondition(!proceed(true, archive, Array(repeating: delta, count: 257))); count += 1
            let otherRole = role == "customer-desk" ? "support-console" : "customer-desk"
            for invalid in [archive.replacingOccurrences(of: role, with: otherRole),
                archive.replacingOccurrences(of: "https://", with: "http://"),
                archive.replacingOccurrences(of: "ongrow.de", with: "ongrow.de.evil.invalid"),
                archive.replacingOccurrences(of: "ongrow.de", with: "user@ongrow.de"),
                archive.replacingOccurrences(of: "ongrow.de", with: "ongrow.de:443"),
                archive + "?token=x", archive + "#fragment", archive + "/nested",
                archive.replacingOccurrences(of: "update-", with: "../update-"),
                archive.replacingOccurrences(of: "update-", with: "%75pdate-"),
                archive.replacingOccurrences(of: "update-", with: "update\\"),
                archive.replacingOccurrences(of: "update-", with: "üpdäte-"),
                String(archive.dropLast(5)) + ".", archive + String(repeating: "a", count: 256)] {
                precondition(!proceed(true, invalid)); count += 1
                precondition(!proceed(true, archive, [invalid])); count += 1
            }
            func allowed(_ environment: [String: String] = [:], _ installed: Bool = true,
                         _ translocated: Bool = false, _ barrier: Bool = true) -> Bool {
                OnGrowUpdatePolicy.mayStart(info, environment: environment, installed: installed,
                    translocated: translocated, sessionBarrierReady: barrier)
            }
            precondition(!allowed()); count += 1
            info["OnGrowUpdateApplyEnabled"] = true
            precondition(allowed()); count += 1
            precondition(!allowed(["GITHUB_ACTIONS": "true"])); count += 1
            precondition(!allowed(["ONGROW_CI_SMOKE_TEST": "1"])); count += 1
            precondition(!allowed([:], false)); count += 1
            precondition(!allowed([:], true, true)); count += 1
            precondition(!allowed([:], true, false, false)); count += 1
            for (key, value) in [("CFBundleIdentifier", "de.foreign.app"),
                ("OnGrowUpdateRole", "unknown"), ("SUPublicEDKey", "broken"),
                ("CFBundleVersion", "0"), ("SUFeedURL", "http://ongrow.de/feed") ] {
                let saved = info[key]
                info[key] = value
                precondition(!allowed()); count += 1
                precondition(!proceed()); count += 1
                info[key] = saved
            }
        }
        precondition(!OnGrowUpdatePolicy.configured([:])); count += 1
        print("macOS update policy: \(count) assertions passed")
#if ONGROW_SPARKLE_DELEGATE_PROBE
        let delegate = OnGrowUpdater()
        for selector in ["updater:mayPerformUpdateCheck:error:",
                         "updater:didFinishLoadingAppcast:",
                         "updater:shouldProceedWithUpdate:updateCheck:error:",
                         "updater:shouldDownloadReleaseNotesForUpdate:",
                         "feedURLStringForUpdater:"] {
            precondition(delegate.responds(to: NSSelectorFromString(selector)))
        }
        print("MACOS_PRODUCTION_DELEGATE_SELECTORS_PASS")
#endif
    }
}
