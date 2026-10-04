import Foundation

@main
struct PolicyTests {
    static func main() {
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
                info[key] = saved
            }
        }
        precondition(!OnGrowUpdatePolicy.configured([:])); count += 1
        print("macOS update policy: \(count) assertions passed")
    }
}
