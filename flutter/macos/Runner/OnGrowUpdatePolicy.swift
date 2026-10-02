import Foundation

enum OnGrowUpdatePolicy {
    static let products = [
        "customer-desk": ("de.ongrow.supportdesk", "OnGROW Support Desk"),
        "support-console": ("de.ongrow.supportconsole", "OnGROW Support Console")
    ]

    static func validFeed(_ value: String, role: String) -> Bool {
        // The exact path prevents cross-product feeds and alternate hosts.
        return products[role] != nil && value == "https://ongrow.de/assets/updates/\(role)/macos-arm64/appcast.xml"
    }

    static func configured(_ info: [String: Any]) -> Bool {
        guard let role = info["OnGrowUpdateRole"] as? String,
              let product = products[role],
              info["CFBundleIdentifier"] as? String == product.0,
              info["CFBundleDisplayName"] as? String == product.1,
              let feed = info["SUFeedURL"] as? String, validFeed(feed, role: role),
              let key = info["SUPublicEDKey"] as? String,
              let bytes = Data(base64Encoded: key), bytes.count == 32,
              bytes.base64EncodedString() == key,
              let sequence = info["OnGrowUpdateSequence"] as? String,
              let number = UInt32(sequence), number > 0, number <= 99_989_999,
              String(number) == sequence,
              info["CFBundleVersion"] as? String == "\(number / 10000 + 1).\(number / 100 % 100).\(number % 100)",
              info["SUVerifyUpdateBeforeExtraction"] as? Bool == true,
              info["SURequireSignedFeed"] as? Bool == true,
              info["SUSignedFeedFailureExpirationInterval"] as? Int == 0,
              info["SUEnableSystemProfiling"] as? Bool == false else { return false }
        return true
    }

    static func mayStart(_ info: [String: Any], environment: [String: String],
                         installed: Bool, translocated: Bool, sessionBarrierReady: Bool) -> Bool {
        if environment["GITHUB_ACTIONS"] != nil || environment["ONGROW_CI_SMOKE_TEST"] != nil {
            return false
        }
        return configured(info) && installed && !translocated && sessionBarrierReady &&
            info["OnGrowUpdateApplyEnabled"] as? Bool == true
    }
}
