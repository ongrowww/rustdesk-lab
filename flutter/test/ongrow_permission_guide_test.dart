import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/ongrow_permission_guide.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('org.rustdesk.rustdesk/host');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('forwards only pane and authoritative permission state', () async {
    final calls = <MethodCall>[];
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return call.method == 'showOnGrowPermissionGuide' ? true : null;
    });
    const status = {
      'screenRecording': false,
      'accessibility': true,
      'inputMonitoring': false,
    };
    expect(await OnGrowPermissionGuide.show('screenRecording', status), isTrue);
    expect(calls.single.arguments, {
      'pane': 'screenRecording',
      'status': status,
    });
    await OnGrowPermissionGuide.update(status);
    expect(calls.last.method, 'updateOnGrowPermissionGuide');
    expect(calls.last.arguments, status);
    await OnGrowPermissionGuide.close();
    expect(calls.last.method, 'closeOnGrowPermissionGuide');
  });

  test(
    'missing native companion falls back to existing permission request',
    () async {
      expect(await OnGrowPermissionGuide.show('screenRecording', {}), isFalse);
      await OnGrowPermissionGuide.update({});
      await OnGrowPermissionGuide.close();
    },
  );

  test(
    'native failure falls back without leaving a pending UI action',
    () async {
      messenger.setMockMethodCallHandler(channel, (_) async {
        throw PlatformException(code: 'settings_unavailable');
      });
      expect(await OnGrowPermissionGuide.show('screenRecording', {}), isFalse);
      await OnGrowPermissionGuide.update({});
      await OnGrowPermissionGuide.close();
    },
  );
}
