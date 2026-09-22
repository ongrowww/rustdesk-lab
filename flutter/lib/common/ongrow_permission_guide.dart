import 'package:flutter/services.dart';

/// Native guidance only. Authorization is still performed by macOS and checked
/// by the same Rust APIs that feed the customer checklist.
class OnGrowPermissionGuide {
  static const _channel = MethodChannel('org.rustdesk.rustdesk/host');

  static Future<bool> show(String pane, Map<String, bool> status) async {
    try {
      return await _channel.invokeMethod<bool>('showOnGrowPermissionGuide', {
            'pane': pane,
            'status': status,
          }) ??
          false;
    } on PlatformException {
      return false;
    } on MissingPluginException {
      return false;
    }
  }

  static Future<void> update(Map<String, bool> status) async {
    try {
      await _channel.invokeMethod<void>('updateOnGrowPermissionGuide', status);
    } on PlatformException {
      // The existing checklist remains the authoritative permission UI.
    } on MissingPluginException {
      // Older native builds and web previews have no companion panel.
    }
  }

  static Future<void> close() async {
    try {
      await _channel.invokeMethod<void>('closeOnGrowPermissionGuide');
    } on PlatformException {
      // Closing guidance must not prevent the customer page from disposing.
    } on MissingPluginException {
      // No native panel exists in the web preview.
    }
  }
}
