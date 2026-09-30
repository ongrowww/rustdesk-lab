import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/desktop/pages/ongrow_support_console.dart';

void main() {
  Future<void> showStatus(
    WidgetTester tester,
    String state, {
    required Size viewport,
    String consoleId = '',
    String error = '',
    double textScale = 1,
  }) async {
    tester.view.physicalSize = viewport;
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(
            context,
          ).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: OnGrowSupportConsole(
          statusProvider: () => jsonEncode({
            'state': state,
            'error': error,
            'console_id': consoleId,
            'control_plane_url': '',
            'signing_public_key': '${List.filled(43, 'A').join()}=',
            'encryption_public_key': '${List.filled(43, 'B').join()}=',
          }),
        ),
      ),
    );
    await tester.pump();
    addTearDown(() async {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }

  Future<void> expectReachable(WidgetTester tester, Finder target) async {
    expect(target, findsOneWidget);
    await tester.ensureVisible(target);
    await tester.pump();
    expect(target.hitTestable(), findsOneWidget);
  }

  Future<void> checkState(
    WidgetTester tester,
    String state,
    Size viewport, {
    double textScale = 1,
  }) async {
    final registration = state == 'registration_required';
    await showStatus(
      tester,
      state,
      viewport: viewport,
      consoleId: registration ? '' : 'test-console-id',
      error: state == 'failed' ? 'session_start_failed' : '',
      textScale: textScale,
    );
    expect(tester.view.physicalSize, viewport);
    expect(tester.takeException(), isNull);
    final header = tester.getRect(find.text('OnGROW Support Console'));
    expect(header.left, greaterThanOrEqualTo(0));
    expect(header.right, lessThanOrEqualTo(viewport.width));
    if (registration) {
      expect(find.text('Konsole registrieren'), findsOneWidget);
      await expectReachable(
        tester,
        find.byTooltip('Signaturschlüssel kopieren'),
      );
      await expectReachable(
        tester,
        find.byTooltip('Verschlüsselungsschlüssel kopieren'),
      );
      await expectReachable(tester, find.text('Support Control öffnen'));
    } else {
      if (state == 'redeeming') {
        expect(
          find.text('Supportverbindung wird vorbereitet …'),
          findsOneWidget,
        );
        expect(find.text('Bereit für Supportverbindungen'), findsNothing);
      } else {
        expect(find.text('Bereit für Supportverbindungen'), findsOneWidget);
      }
      await expectReachable(
        tester,
        find.text('Geräte in Support Control öffnen'),
      );
      if (state == 'failed') {
        await expectReachable(
          tester,
          find.textContaining('session_start_failed'),
        );
      }
    }
    expect(tester.takeException(), isNull);
  }

  const states = ['registration_required', 'idle', 'redeeming', 'failed'];
  for (final viewport in [
    const Size(600, 450),
    const Size(1400, 1000),
    const Size(800, 600),
  ]) {
    for (final state in states) {
      testWidgets('$state remains usable at $viewport', (tester) async {
        await checkState(tester, state, viewport);
      });
    }
  }

  for (final viewport in [const Size(320, 600), const Size(600, 450)]) {
    for (final state in states) {
      testWidgets('$state reflows at $viewport with 200% text', (tester) async {
        await checkState(tester, state, viewport, textScale: 2);
      });
    }
  }

  testWidgets('short window has a visible scrollbar and reachable actions', (
    tester,
  ) async {
    await showStatus(
      tester,
      'registration_required',
      viewport: const Size(600, 450),
    );
    final scrollView = tester.widget<SingleChildScrollView>(
      find.byType(SingleChildScrollView),
    );
    final scrollbar = tester.widget<Scrollbar>(
      find.byKey(const ValueKey('ongrow-console-scrollbar')),
    );
    expect(scrollbar.thumbVisibility, isTrue);
    expect(scrollbar.controller, same(scrollView.controller));
    expect(scrollView.controller!.position.maxScrollExtent, greaterThan(0));
    await expectReachable(tester, find.text('Support Control öffnen'));
    expect(scrollView.controller!.offset, greaterThan(0));
    expect(tester.takeException(), isNull);
  });
}
