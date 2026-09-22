import Cocoa

// Native companion to the Flutter permission checklist. Dragging supplies the
// installed app URL only; System Settings remains responsible for authorization.
// Status comes from the existing Rust checks, never from a successful drop.
enum OnGrowPermissionPane: String, CaseIterable {
    case screenRecording, accessibility, inputMonitoring

    var title: String {
        switch self {
        case .screenRecording: return "Bildschirmaufnahme"
        case .accessibility: return "Bedienungshilfen"
        case .inputMonitoring: return "Eingabeüberwachung"
        }
    }

    var anchor: String {
        switch self {
        case .screenRecording: return "Privacy_ScreenCapture"
        case .accessibility: return "Privacy_Accessibility"
        case .inputMonitoring: return "Privacy_ListenEvent"
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
    private var nextButton: NSButton?
    private var keyboardEntry: NSButton?
    private var lastGranted: Bool?
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
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 350),
            styleMask: [.titled, .closable, .utilityWindow, .nonactivatingPanel],
            backing: .buffered, defer: false)
        panel.title = "\(pane.title) einrichten"
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
        return true
    }

    func update(status: [String: Bool]) {
        self.status = status
        guard let pane = pane, panel != nil else { return }
        let granted = status[pane.rawValue] == true
        guard lastGranted != granted else { return }
        lastGranted = granted
        statusLabel?.stringValue = granted
            ? "✓ Erlaubt · Die App hat die Freigabe erkannt."
            : "Noch nicht erkannt. Falls macOS es verlangt, beende und öffne die App erneut."
        statusLabel?.textColor = granted ? .systemGreen : .secondaryLabelColor
        nextButton?.title = granted ? "Weiter" : "Zurück zur App"
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
        statusLabel = nil
        nextButton = nil
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
        let title = label("Ziehe das App-Icon in die Liste", bold: true)
        let instructions = label("Ziehe OnGROW Support Desk in die Liste für \(pane.title). Ist die App schon eingetragen, schalte sie dort ein. Bestätige anschließend die Rückfrage von macOS.")
        let drag = OnGrowAppDragView(appURL: Bundle.main.bundleURL)
        drag.onDrag = { [weak self] active in
            self?.dragging = active
            self?.panel?.ignoresMouseEvents = active
        }
        let state = label("Noch nicht erkannt. Falls macOS es verlangt, beende und öffne die App erneut.")
        state.textColor = .secondaryLabelColor
        statusLabel = state
        let alternative = OnGrowGuideButton(title: "Ohne Ziehen einrichten", target: self, action: #selector(showAlternative))
        alternative.bezelStyle = .rounded
        let next = OnGrowGuideButton(title: "Zurück zur App", target: self, action: #selector(continueSetup))
        next.bezelStyle = .rounded
        nextButton = next
        keyboardEntry = alternative
        root.nextKeyView = alternative
        alternative.nextKeyView = next
        next.nextKeyView = alternative
        for view in [title, instructions, drag, state, alternative, next] {
            stack.addArrangedSubview(view)
        }
        for view in [title, instructions, drag, state] {
            view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }
        drag.heightAnchor.constraint(equalToConstant: 66).isActive = true
        // Native semantic colors follow light/dark appearance and contrast.
        // No custom motion, no time-limited instructions, no extra permissions.
        return root
    }

    @objc private func showAlternative() {
        guard let pane = pane, let url = pane.settingsURL else { return }
        let alert = NSAlert()
        alert.messageText = "App ohne Ziehen hinzufügen"
        alert.informativeText = "Wähle in der Liste für \(pane.title) das Pluszeichen. Drücke im Dateidialog ⇧⌘G und füge den App-Pfad mit ⌘V ein. Wähle die App aus und aktiviere ihren Schalter. Ist sie bereits eingetragen, aktiviere nur den Schalter."
        alert.addButton(withTitle: "App-Pfad kopieren")
        alert.addButton(withTitle: "Abbrechen")
        guard let panel = panel else { return }
        alert.beginSheetModal(for: panel) { response in
            if response == .alertFirstButtonReturn {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(Bundle.main.bundlePath, forType: .string)
                NSWorkspace.shared.open(url)
            }
        }
    }

    @objc private func continueSetup() {
        guard let current = pane, status[current.rawValue] == true else {
            dismiss(restoreFocus: true)
            return
        }
        // Advancing is explicit so Settings never changes while a user is
        // interacting with a system authentication or relaunch dialog.
        if let next = OnGrowPermissionPane.allCases.first(where: { status[$0.rawValue] != true }) {
            let previousOwner = owner
            if !show(pane: next, status: status, owner: previousOwner) {
                dismiss(restoreFocus: true)
            }
        } else {
            dismiss(restoreFocus: true)
        }
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

private final class OnGrowGuideButton: NSButton {
    // Keep the alternative usable even when macOS's optional keyboard
    // navigation setting is off. Native button focus and activation remain.
    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { true }
}

private final class OnGrowAppDragView: NSView, NSDraggingSource {
    private let appURL: URL
    private var start: NSPoint?
    var onDrag: ((Bool) -> Void)?

    init(appURL: URL) {
        self.appURL = appURL
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("OnGROW Support Desk. Ziehbares App-Icon. Alternativ: Ohne Ziehen einrichten.")
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
    }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func mouseDown(with event: NSEvent) { start = event.locationInWindow }
    override func mouseUp(with event: NSEvent) { start = nil }
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
