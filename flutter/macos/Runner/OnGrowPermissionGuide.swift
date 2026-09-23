import Cocoa
import AVFoundation

// Native companion to the Flutter permission checklist. Dragging supplies the
// installed app URL only; System Settings remains responsible for authorization.
// Status comes from the existing Rust checks, never from a successful drop.
enum OnGrowPermissionPane: String, CaseIterable {
    case screenRecording, accessibility, inputMonitoring, microphone

    var title: String {
        switch self {
        case .screenRecording: return "Bildschirmaufnahme"
        case .accessibility: return "Bedienungshilfen"
        case .inputMonitoring: return "Eingabeüberwachung"
        case .microphone: return "Mikrofon"
        }
    }

    var anchor: String {
        switch self {
        case .screenRecording: return "Privacy_ScreenCapture"
        case .accessibility: return "Privacy_Accessibility"
        case .inputMonitoring: return "Privacy_ListenEvent"
        case .microphone: return "Privacy_Microphone"
        }
    }

    var settingsURL: URL? {
        URL(string: "x-apple.systempreferences:com.apple.preference.security?\(anchor)")
    }
}

final class OnGrowPermissionGuide: NSObject, NSWindowDelegate {
    static let shared = OnGrowPermissionGuide()
    private var panel: OnGrowPermissionPanel?
    private var pane: OnGrowPermissionPane?
    private var status: [String: Bool] = [:]
    private var timer: Timer?
    private weak var owner: NSWindow?
    private var statusLabel: NSTextField?
    private var keyboardEntry: NSView?
    private var lastGranted: Bool?
    private var grantedAt: Date?
    private var openedAt = Date()
    private var dragging = false

    // Called only on the Flutter host channel's main thread.
    func show(pane: OnGrowPermissionPane, status: [String: Bool], owner: NSWindow?) -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        guard Bundle.main.bundleURL.pathExtension == "app",
              let url = pane.settingsURL,
              NSWorkspace.shared.open(url) else { return false }
        dismiss(restoreFocus: false)
        self.pane = pane
        self.status = status
        self.owner = owner
        openedAt = Date()

        let panel = OnGrowPermissionPanel(
            contentRect: NSRect(x: 0, y: 0, width: 340, height: 200),
            styleMask: [.titled, .closable, .utilityWindow, .nonactivatingPanel],
            backing: .buffered, defer: false)
        panel.title = pane.title
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.appearance = owner?.appearance
        panel.level = .floating
        panel.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
        panel.delegate = self
        panel.onEscape = { [weak self] in self?.dismiss(restoreFocus: true) }
        panel.contentView = makeContent(pane: pane)
        panel.initialFirstResponder = keyboardEntry
        self.panel = panel
        panel.center()
        update(status: status)
        panel.orderFrontRegardless()
        followSettings()
        timer = Timer(timeInterval: 0.4, repeats: true) { [weak self] _ in
            self?.followSettings()
        }
        if let timer = timer { RunLoop.main.add(timer, forMode: .common) }
        if pane == .microphone && AVCaptureDevice.authorizationStatus(for: .audio) == .notDetermined {
            NSApp.activate(ignoringOtherApps: true)
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] granted in
                DispatchQueue.main.async {
                    guard let self = self, self.pane == .microphone else { return }
                    if granted { self.dismiss(restoreFocus: true) }
                    else {
                        self.statusLabel?.stringValue = "Nicht erlaubt · Fernzugriff funktioniert auch ohne Mikrofon."
                        self.statusLabel?.isHidden = false
                    }
                }
            }
        }
        return true
    }

    func update(status: [String: Bool]) {
        self.status = status
        guard let pane = pane, panel != nil else { return }
        let granted = status[pane.rawValue] == true
        guard lastGranted != granted else { return }
        lastGranted = granted
        grantedAt = granted ? Date() : nil
        statusLabel?.stringValue = granted
            ? "✓ Erlaubt"
            : ""
        statusLabel?.isHidden = !granted
        if pane == .microphone && !granted && AVCaptureDevice.authorizationStatus(for: .audio) != .notDetermined {
            statusLabel?.stringValue = "Nicht erlaubt · Fernzugriff funktioniert auch ohne Mikrofon."
            statusLabel?.isHidden = false
        }
        statusLabel?.textColor = granted ? .systemGreen : .secondaryLabelColor
        if let label = statusLabel {
            NSAccessibility.post(element: label, notification: .valueChanged)
        }
    }

    func dismiss(restoreFocus: Bool) {
        timer?.invalidate()
        timer = nil
        panel?.orderOut(nil)
        panel?.delegate = nil
        panel = nil
        pane = nil
        lastGranted = nil
        grantedAt = nil
        statusLabel = nil
        keyboardEntry = nil
        dragging = false
        if restoreFocus, let owner = owner {
            NSApp.activate(ignoringOtherApps: true)
            owner.makeKeyAndOrderFront(nil)
        }
        owner = nil
    }

    func windowWillClose(_ notification: Notification) {
        dismiss(restoreFocus: true)
    }

    private func label(_ text: String, bold: Bool = false) -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = bold ? .boldSystemFont(ofSize: 15) : .systemFont(ofSize: 13)
        label.textColor = .labelColor
        label.setContentCompressionResistancePriority(.required, for: .vertical)
        return label
    }

    func makeContent(pane: OnGrowPermissionPane) -> NSView {
        let root = NSView()
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20),
            stack.topAnchor.constraint(equalTo: root.topAnchor, constant: 18),
            stack.bottomAnchor.constraint(lessThanOrEqualTo: root.bottomAnchor, constant: -18)
        ])
        let instructions = label(pane == .microphone
            ? "Erlaube das Mikrofon für Gespräche im Support. Du kannst auch ablehnen."
            : "Ziehe das App-Icon in die Liste „\(pane.title)“ und schalte es ein.", bold: true)
        let drag = OnGrowAppDragView(appURL: Bundle.main.bundleURL)
        drag.onActivate = { [weak self, weak drag] in
            self?.showAlternative(pane: pane, window: drag?.window)
        }
        drag.onDrag = { [weak self] active in
            self?.dragging = active
            self?.panel?.ignoresMouseEvents = active
        }
        let state = label("")
        state.isHidden = true
        state.textColor = .secondaryLabelColor
        statusLabel = state
        keyboardEntry = pane == .microphone ? nil : drag
        root.nextKeyView = keyboardEntry
        drag.nextKeyView = drag
        let views: [NSView] = pane == .microphone ? [instructions, state] : [instructions, drag, state]
        for view in views {
            stack.addArrangedSubview(view)
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        drag.heightAnchor.constraint(equalToConstant: 66).isActive = true
        // Native semantic colors follow light/dark appearance and contrast.
        // No custom motion, no time-limited instructions, no extra permissions.
        return root
    }

    private func showAlternative(pane: OnGrowPermissionPane, window: NSWindow?) {
        guard let url = pane.settingsURL, let window = window else { return }
        let alert = NSAlert()
        alert.messageText = "App ohne Ziehen hinzufügen"
        alert.informativeText = "Wähle in der Liste für \(pane.title) das Pluszeichen. Drücke im Dateidialog ⇧⌘G und füge den App-Pfad mit ⌘V ein. Wähle die App aus und aktiviere ihren Schalter. Ist sie bereits eingetragen, aktiviere nur den Schalter."
        alert.addButton(withTitle: "App-Pfad kopieren")
        alert.addButton(withTitle: "Abbrechen")
        alert.beginSheetModal(for: window) { response in
            if response == .alertFirstButtonReturn {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(Bundle.main.bundlePath, forType: .string)
                NSWorkspace.shared.open(url)
            }
        }
    }

    private func continueSetup() {
        guard let current = pane, status[current.rawValue] == true else {
            dismiss(restoreFocus: true)
            return
        }
        if let next = Self.nextPane(after: current, status: status) {
            let previousOwner = owner
            if !show(pane: next, status: status, owner: previousOwner) {
                dismiss(restoreFocus: true)
            }
        } else {
            dismiss(restoreFocus: true)
        }
    }

    static func nextPane(after current: OnGrowPermissionPane, status: [String: Bool]) -> OnGrowPermissionPane? {
        guard status[current.rawValue] == true else { return current }
        return OnGrowPermissionPane.allCases.first { status[$0.rawValue] != true }
    }

    private func followSettings() {
        guard let panel = panel, !dragging, panel.attachedSheet == nil else { return }
        panel.appearance = owner?.appearance
        guard let settings = NSRunningApplication.runningApplications(
            withBundleIdentifier: "com.apple.systempreferences").first else {
            if Date().timeIntervalSince(openedAt) > 5 { dismiss(restoreFocus: false) }
            return
        }
        let foreground = NSWorkspace.shared.frontmostApplication?.processIdentifier
        guard foreground == settings.processIdentifier || foreground == ProcessInfo.processInfo.processIdentifier else {
            panel.orderOut(nil)
            return
        }
        // Never navigate during a drag or our keyboard-help sheet. Only a real
        // permission check can advance the guide, never a completed drop.
        if let pane = pane, status[pane.rawValue] == true,
           let grantedAt = grantedAt, Date().timeIntervalSince(grantedAt) >= 0.8 {
            continueSetup()
            return
        }
        // Window geometry only. No AX trust, screenshots or synthetic clicks.
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        let frames = windows.compactMap { item -> CGRect? in
            guard (item[kCGWindowOwnerPID as String] as? Int32) == settings.processIdentifier,
                  (item[kCGWindowLayer as String] as? Int) == 0,
                  let bounds = item[kCGWindowBounds as String] as? NSDictionary,
                  let frame = CGRect(dictionaryRepresentation: bounds),
                  frame.width > 300, frame.height > 200 else { return nil }
            return frame
        }
        guard let frame = frames.max(by: { $0.width * $0.height < $1.width * $1.height }),
              let primary = NSScreen.screens.first else {
            if Date().timeIntervalSince(openedAt) > 5 { panel.orderOut(nil) }
            return
        }
        let settingsFrame = NSRect(x: frame.minX, y: primary.frame.maxY - frame.maxY,
                                   width: frame.width, height: frame.height)
        let screen = NSScreen.screens.max { left, right in
            let a = left.frame.intersection(settingsFrame)
            let b = right.frame.intersection(settingsFrame)
            return (a.isNull ? 0 : a.width * a.height) < (b.isNull ? 0 : b.width * b.height)
        } ?? primary
        let target = Self.companionFrame(settings: settingsFrame, visible: screen.visibleFrame, size: panel.frame.size)
        if panel.frame != target { panel.setFrame(target, display: true) }
        panel.orderFrontRegardless()
    }

    static func companionFrame(settings: NSRect, visible: NSRect, size: NSSize) -> NSRect {
        // Prefer beside Settings, otherwise below. Clamp to the current display
        // so small screens and negative-origin secondary monitors remain usable.
        var x = settings.maxX + 12
        var y = settings.maxY - size.height
        if x + size.width > visible.maxX {
            x = settings.minX - size.width - 12
            if x < visible.minX {
                x = settings.midX - size.width / 2
                y = settings.minY - size.height - 12
            }
        }
        return NSRect(x: max(visible.minX, min(x, visible.maxX - size.width)),
                      y: max(visible.minY, min(y, visible.maxY - size.height)),
                      width: size.width, height: size.height)
    }
}

private final class OnGrowPermissionPanel: NSPanel {
    var onEscape: (() -> Void)?
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onEscape?() }
}

private final class OnGrowAppDragView: NSView, NSDraggingSource {
    private let appURL: URL
    private var start: NSPoint?
    var onDrag: ((Bool) -> Void)?
    var onActivate: (() -> Void)?
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { true }
    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() { NSBezierPath(roundedRect: bounds, xRadius: 10, yRadius: 10).fill() }
    override func becomeFirstResponder() -> Bool { needsDisplay = true; return true }
    override func resignFirstResponder() -> Bool { needsDisplay = true; return true }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 49 { onActivate?() }
        else { super.keyDown(with: event) }
    }
    override func accessibilityPerformPress() -> Bool { onActivate?(); return true }

    init(appURL: URL) {
        self.appURL = appURL
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("OnGROW Support Desk hinzufügen")
        setAccessibilityHelp("In die Berechtigungsliste ziehen oder mit Eingabetaste die Tastaturanleitung öffnen.")
        focusRingType = .exterior
    }
    required init?(coder: NSCoder) { return nil }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlBackgroundColor.setFill()
        let background = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 10, yRadius: 10)
        background.fill()
        NSColor.separatorColor.setStroke()
        background.stroke()
        NSWorkspace.shared.icon(forFile: appURL.path).draw(in: NSRect(x: 12, y: 9, width: 48, height: 48))
        ("OnGROW Support Desk" as NSString).draw(at: NSPoint(x: 72, y: 35), withAttributes: [
            .font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor])
        ("In die Berechtigungsliste ziehen" as NSString).draw(at: NSPoint(x: 72, y: 16), withAttributes: [
            .font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.secondaryLabelColor])
        if window?.firstResponder === self {
            NSGraphicsContext.saveGraphicsState()
            NSFocusRingPlacement.only.set()
            background.fill()
            NSGraphicsContext.restoreGraphicsState()
        }
    }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func mouseDown(with event: NSEvent) { start = event.locationInWindow }
    override func mouseUp(with event: NSEvent) {
        if start != nil { onActivate?() }
        start = nil
    }
    override func mouseDragged(with event: NSEvent) {
        guard let start = start,
              hypot(event.locationInWindow.x - start.x, event.locationInWindow.y - start.y) > 4 else { return }
        self.start = nil
        let item = NSDraggingItem(pasteboardWriter: appURL as NSURL)
        let point = convert(event.locationInWindow, from: nil)
        item.setDraggingFrame(NSRect(x: point.x - 24, y: point.y - 24, width: 48, height: 48),
                              contents: NSWorkspace.shared.icon(forFile: appURL.path))
        beginDraggingSession(with: [item], event: event, source: self)
    }
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation { .copy }
    func draggingSession(_ session: NSDraggingSession, willBeginAt screenPoint: NSPoint) { onDrag?(true) }
    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) { onDrag?(false) }
}
