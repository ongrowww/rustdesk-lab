import CryptoKit
import Foundation

@main
struct EphemeralKeys {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else { throw NSError(domain: "probe", code: 1) }
        let key = Curve25519.Signing.PrivateKey()
        // Sparkle 2.10 accepts the raw 32-byte seed, not a libsodium keypair.
        let secret = key.rawRepresentation
        let privateURL = URL(fileURLWithPath: CommandLine.arguments[1])
        guard FileManager.default.createFile(atPath: privateURL.path,
            contents: Data(secret.base64EncodedString().utf8),
            attributes: [.posixPermissions: 0o600]) else { throw NSError(domain: "probe", code: 2) }
        try Data(key.publicKey.rawRepresentation.base64EncodedString().utf8)
            .write(to: URL(fileURLWithPath: CommandLine.arguments[2]), options: .withoutOverwriting)
    }
}
