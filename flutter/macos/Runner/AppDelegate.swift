import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
    private let onGrowUpdater = OnGrowUpdater()
    var launched = false;
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
      dummy_method_to_enforce_bundling()
    // https://github.com/leanflutter/window_manager/issues/214
    return false
  }
    
    override func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        if (launched) {
            handle_applicationShouldOpenUntitledFile();
        }
        return true
    }
    
    override func applicationDidFinishLaunching(_ aNotification: Notification) {
        launched = true;
        onGrowUpdater.startIfSafe()
        NSApplication.shared.activate(ignoringOtherApps: true);
    }
}
