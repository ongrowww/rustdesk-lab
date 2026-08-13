import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';

const _snapshot = OnGrowSupportSnapshot(
  supportId: '682 419 753',
  ready: true,
  canRecordScreen: true,
  isProcessTrusted: false,
  canMonitorInput: false,
  canRecordAudio: false,
  canAcceptIncomingConnections: false,
  version: '1.4.9',
);

Widget _app(
  OnGrowSupportActions actions, {
  OnGrowSupportSnapshot snapshot = _snapshot,
}) {
  return MaterialApp(
    theme: ThemeData(
      colorScheme: ColorScheme.fromSeed(seedColor: ongrowViolet),
      useMaterial3: true,
    ),
    home: Scaffold(
      body: SizedBox(
        width: 1120,
        height: 620,
        child: OnGrowSupportView(
          snapshot: snapshot,
          actions: actions,
        ),
      ),
    ),
  );
}

OnGrowSupportActions _actions({
  Future<void> Function()? requestSupport,
  VoidCallback? openSettings,
  Future<void> Function()? openNetworkSettings,
  Future<OnGrowSupportSnapshot> Function()? refresh,
  Future<void> Function()? enableUnattended,
  Future<void> Function()? revokeUnattended,
}) {
  return OnGrowSupportActions(
    copySupportId: () async {},
    requestSupport: requestSupport ?? () async {},
    openSettings: openSettings ?? () {},
    requestScreenRecording: () async {},
    requestAccessibility: () async {},
    requestInputMonitoring: () async {},
    requestMicrophone: () async {},
    openNetworkSettings: openNetworkSettings ?? () async {},
    refresh: refresh ?? () async => _snapshot,
    enableUnattended: enableUnattended ?? () async {},
    revokeUnattended: revokeUnattended ?? () async {},
  );
}

void main() {
  testWidgets('requires explicit confirmation before unattended access',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    var enabled = 0;
    await tester.pumpWidget(
      _app(
        _actions(enableUnattended: () async => enabled += 1),
      ),
    );

    await tester.ensureVisible(find.text('Zugriff für OnGROW freigeben'));
    await tester.tap(find.text('Zugriff für OnGROW freigeben'));
    await tester.pumpAndSettle();
    expect(find.text('Unbeaufsichtigten Zugriff freigeben?'), findsOneWidget);
    expect(enabled, 0);

    await tester.tap(find.text('Sicher freigeben'));
    await tester.pumpAndSettle();
    expect(enabled, 1);
  });

  testWidgets('requires confirmation before revoking unattended access',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    var revoked = 0;
    await tester.pumpWidget(
      _app(
        _actions(revokeUnattended: () async => revoked += 1),
        snapshot: _snapshot.copyWith(
          unattendedStatus: OnGrowUnattendedStatus.enabled,
        ),
      ),
    );

    await tester.ensureVisible(find.text('Zugriff widerrufen'));
    await tester.tap(find.text('Zugriff widerrufen'));
    await tester.pumpAndSettle();
    expect(find.text('Zugriff wirklich widerrufen?'), findsOneWidget);
    expect(revoked, 0);

    await tester.tap(find.text('Zugriff widerrufen').last);
    await tester.pumpAndSettle();
    expect(revoked, 1);
  });

  testWidgets('never overwrites an existing permanent password',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    var settingsOpened = 0;
    var enableAttempts = 0;
    await tester.pumpWidget(
      _app(
        _actions(
          openSettings: () => settingsOpened += 1,
          enableUnattended: () async => enableAttempts += 1,
        ),
        snapshot: _snapshot.copyWith(
          unattendedStatus: OnGrowUnattendedStatus.actionRequired,
          unattendedError: 'existing_password_conflict',
        ),
      ),
    );

    await tester.ensureVisible(find.text('RustDesk-Einstellungen öffnen'));
    await tester.tap(find.text('RustDesk-Einstellungen öffnen'));
    await tester.pumpAndSettle();

    expect(settingsOpened, 1);
    expect(enableAttempts, 0);
    expect(find.text('Unbeaufsichtigten Zugriff freigeben?'), findsNothing);
  });

  testWidgets('prepares the support email from the contact modal',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    var emailRequests = 0;
    await tester.pumpWidget(
      _app(
        _actions(
          requestSupport: () async {
            emailRequests += 1;
          },
        ),
      ),
    );

    await tester.tap(find.text('Support anfordern'));
    await tester.pumpAndSettle();

    expect(find.text('OnGROW GmbH Kundensupport'), findsOneWidget);
    expect(find.text('support@ongrow.de'), findsOneWidget);
    expect(find.textContaining('Support-ID 682 419 753'), findsOneWidget);
    expect(find.text('E-Mail-App öffnen'), findsOneWidget);

    await tester.tap(find.text('E-Mail-App öffnen'));
    await tester.pumpAndSettle();

    expect(emailRequests, 1);
    expect(find.text('support@ongrow.de'), findsNothing);
  });

  testWidgets('offers the macOS network settings help step', (tester) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    var networkRequests = 0;
    await tester.pumpWidget(
      _app(
        _actions(
          openNetworkSettings: () async {
            networkRequests += 1;
          },
        ),
      ),
    );

    expect(find.text('Netzwerkzugriff'), findsOneWidget);
    await tester.ensureVisible(find.text('Prüfen →'));
    await tester.tap(find.text('Prüfen →'));
    await tester.pumpAndSettle();

    await tester.ensureVisible(find.text('Netzwerkeinstellungen öffnen'));
    await tester.tap(find.text('Netzwerkeinstellungen öffnen'));
    await tester.pumpAndSettle();

    expect(networkRequests, 1);
  });

  testWidgets('shows permitted incoming connections as granted',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _app(
        _actions(),
        snapshot: _snapshot.copyWith(canAcceptIncomingConnections: true),
      ),
    );

    expect(find.text('Prüfen →'), findsNothing);
    expect(find.text('✓ Erlaubt'), findsNWidgets(2));
  });

  testWidgets('refreshes granted permissions while the help dialog is open',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final granted = _snapshot.copyWith(
      canRecordScreen: true,
      isProcessTrusted: true,
      canMonitorInput: true,
      canRecordAudio: true,
      canAcceptIncomingConnections: true,
    );
    var refreshed = false;
    await tester.pumpWidget(
      _app(
        _actions(
          refresh: () async => refreshed ? granted : _snapshot,
        ),
      ),
    );

    await tester.tap(find.text('Einrichtungshilfe'));
    await tester.pumpAndSettle();
    expect(find.text('Jetzt einrichten'), findsWidgets);

    refreshed = true;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();

    expect(find.text('Erledigt'), findsNWidgets(5));
  });
}
