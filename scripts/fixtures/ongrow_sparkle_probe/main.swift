import Cocoa
import Sparkle

// The marker is compiled into different v0/v1/v2 executable payloads.
// sparkle-cli updates its isolated bundle in the CI temporary directory.
@main
struct Probe {
    static func main() {
        if CommandLine.arguments.contains("--marker") {
            #if PROBE_V2
            print("2")
            #elseif PROBE_V0
            print("0")
            #else
            print("1")
            #endif
        }
    }
}
