import Cocoa

@main
struct PermissionGuideTests {
    static func main() {
        precondition(OnGrowPermissionPane(rawValue: "fullDiskAccess") == nil)
        precondition(OnGrowPermissionPane(rawValue: "network") == nil)
        let expectedLinks: [OnGrowPermissionPane: String] = [
            .screenRecording: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
            .accessibility: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            .inputMonitoring: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
            .microphone: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        ]
        for pane in OnGrowPermissionPane.allCases {
            precondition(pane.settingsURL?.scheme == "x-apple.systempreferences")
            // Older Foundation treats this non-hierarchical Settings URL as
            // opaque and returns nil for .query. Verify the complete URL that
            // NSWorkspace receives instead, including the exact privacy pane.
            precondition(pane.settingsURL?.absoluteString == expectedLinks[pane])
        }
        precondition(OnGrowPermissionGuide.nextPane(after: .screenRecording, status: [:]) == .screenRecording)
        precondition(OnGrowPermissionGuide.nextPane(after: .screenRecording, status: ["screenRecording": true]) == .accessibility)
        precondition(OnGrowPermissionGuide.nextPane(after: .accessibility, status: ["screenRecording": true, "accessibility": true]) == .inputMonitoring)
        precondition(OnGrowPermissionGuide.nextPane(after: .inputMonitoring, status: ["screenRecording": true, "accessibility": true, "inputMonitoring": true]) == .microphone)
        precondition(OnGrowPermissionGuide.nextPane(after: .microphone, status: ["screenRecording": true, "accessibility": true, "inputMonitoring": true, "microphone": true]) == nil)
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
            let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 340, height: 200),
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
