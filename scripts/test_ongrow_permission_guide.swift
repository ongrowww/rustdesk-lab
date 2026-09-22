import Cocoa

@main
struct PermissionGuideTests {
    static func main() {
        precondition(OnGrowPermissionPane(rawValue: "fullDiskAccess") == nil)
        precondition(OnGrowPermissionPane(rawValue: "network") == nil)
        for pane in OnGrowPermissionPane.allCases {
            precondition(pane.settingsURL?.scheme == "x-apple.systempreferences")
            precondition(pane.settingsURL?.query == pane.anchor)
        }
        let size = NSSize(width: 400, height: 378)
        let displays = [NSRect(x: 0, y: 30, width: 1440, height: 850),
                        NSRect(x: -1920, y: 0, width: 1920, height: 1080),
                        NSRect(x: 0, y: 900, width: 1280, height: 800),
                        NSRect(x: 0, y: 0, width: 800, height: 600)]
        for display in displays {
            for offset in [CGFloat(0), 100, display.width - 600] {
                let settings = NSRect(x: display.minX + offset, y: display.minY + 50, width: 600, height: 550)
                let result = OnGrowPermissionGuide.companionFrame(settings: settings, visible: display, size: size)
                precondition(display.contains(result), "Guide must remain within the visible screen")
            }
        }
        print("Permission guide: pane allowlist, Settings links and 12 display-placement cases passed")

        // Optional visual-only harness: no Settings launch, permission requests,
        // screen capture, network access or modification of the installed app.
        if CommandLine.arguments.contains("--preview") {
            let app = NSApplication.shared
            app.setActivationPolicy(.regular)
            let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 400, height: 350),
                                  styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.title = "OnGROW Permission Guide · UI Test"
            window.appearance = NSAppearance(named: CommandLine.arguments.contains("--dark") ? .darkAqua : .aqua)
            window.contentView = OnGrowPermissionGuide.shared.makeContent(pane: .screenRecording)
            window.initialFirstResponder = window.contentView?.nextKeyView
            window.makeKeyAndOrderFront(nil)
            window.makeFirstResponder(window.initialFirstResponder)
            app.activate(ignoringOtherApps: true)
            app.run()
        }
    }
}
