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

    static func validArchive(_ value: String, role: String) -> Bool {
        let prefix = "https://ongrow.de/assets/updates/\(role)/macos-arm64/"
        guard products[role] != nil, value.utf8.count <= 2048,
              value.hasPrefix(prefix) else { return false }
        // Validate the raw enclosure, not a decoded/normalized URL. Stock
        // Sparkle may still follow HTTPS redirects after this initial request.
        let filename = value.dropFirst(prefix.count)
        guard !filename.isEmpty, filename.utf8.count <= 255,
              filename != ".", filename != "..", !filename.hasSuffix(".") else { return false }
        return filename.utf8.allSatisfy { byte in
            (byte >= 65 && byte <= 90) || (byte >= 97 && byte <= 122) ||
            (byte >= 48 && byte <= 57) || byte == 45 || byte == 95 || byte == 46
        }
    }

    static func mayProceed(_ info: [String: Any], feedSigned: Bool,
                           archive: String?, deltaArchives: [String?],
                           installationType: String) -> Bool {
        guard configured(info), feedSigned, installationType == "application",
              let role = info["OnGrowUpdateRole"] as? String,
              let archive = archive, validArchive(archive, role: role),
              deltaArchives.count <= 256 else { return false }
        // The selected delta and its full-archive fallback both come from the
        // signed appcast. Never allow a secondary candidate outside the role.
        return deltaArchives.allSatisfy { candidate in
            guard let candidate = candidate else { return false }
            return validArchive(candidate, role: role)
        }
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
