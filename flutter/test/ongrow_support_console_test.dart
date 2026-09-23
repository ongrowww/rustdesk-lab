import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/desktop/pages/ongrow_support_console.dart';

void main() {
  Future<void> showStatus(
    WidgetTester tester,
    String state, {
    String consoleId = '',
    String error = '',
  }) async {
    tester.view.physicalSize = const Size(1400, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        home: OnGrowSupportConsole(
          statusProvider: () => jsonEncode({
            'state': state,
            'error': error,
            'console_id': consoleId,
            'control_plane_url': '',
          }),
        ),
      ),
    );
    await tester.pump();
  }

  testWidgets('registration state exposes public-key instructions', (
    tester,
  ) async {
    await showStatus(tester, 'registration_required');
    expect(find.text('Konsole registrieren'), findsOneWidget);
    expect(find.text('Signaturschlüssel'), findsOneWidget);
  });

  testWidgets('registered console shows waiting state', (tester) async {
    await showStatus(tester, 'idle', consoleId: 'test-console-id');
    expect(find.text('Bereit für Supportverbindungen'), findsOneWidget);
  });

  testWidgets('redeem is not shown as a started connection', (tester) async {
    await showStatus(tester, 'redeeming', consoleId: 'test-console-id');
    expect(find.text('Supportverbindung wird vorbereitet …'), findsOneWidget);
    expect(find.text('Bereit für Supportverbindungen'), findsNothing);
  });

  testWidgets('failure remains visible', (tester) async {
    await showStatus(
      tester,
      'failed',
      consoleId: 'test-console-id',
      error: 'session_start_failed',
    );
    expect(find.textContaining('session_start_failed'), findsOneWidget);
  });
}
