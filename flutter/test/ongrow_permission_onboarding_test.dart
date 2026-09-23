import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/ongrow_permission_onboarding.dart';

void main() {
  test('microphone is requested after the three remote-control permissions', () {
    expect(OnGrowPermissionOnboarding.nextStep(screen: true, accessibility: true,
        input: true, microphoneHandled: false), 4);
    expect(OnGrowPermissionOnboarding.nextStep(screen: true, accessibility: true,
        input: true, microphoneHandled: true), isNull);
  });
  int? step(String saved, bool screen, bool accessibility, bool input) =>
      OnGrowPermissionOnboarding.startupStep(
        saved: saved, screen: screen, accessibility: accessibility, input: input);

  test('fresh install starts with screen recording, not accessibility', () {
    expect(step('', false, false, false), 0);
  });
  test('system relaunch resumes at the first actually missing permission', () {
    expect(step('active', true, false, false), 1);
    expect(step('active', true, true, false), 2);
    expect(step('active', true, true, true), isNull);
  });
  test('stale progress never substitutes for macOS authorization', () {
    expect(step('active', false, true, true), 0);
    expect(step('unknown-future-state', false, false, false), 0);
  });
  test('completed setup does not interrupt later launches', () {
    expect(step('complete', true, true, true), isNull);
    expect(step('complete', false, true, true), isNull);
  });
  test('existing grants skip onboarding on an upgrade', () {
    expect(step('', true, true, true), isNull);
  });
}
