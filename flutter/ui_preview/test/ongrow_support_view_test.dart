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
  Future<void> Function()? openNetworkSettings,
  Future<OnGrowSupportSnapshot> Function()? refresh,
}) {
  return OnGrowSupportActions(
    copySupportId: () async {},
    requestSupport: requestSupport ?? () async {},
    openSettings: () {},
    requestScreenRecording: () async {},
    requestAccessibility: () async {},
    requestInputMonitoring: () async {},
    requestMicrophone: () async {},
    openNetworkSettings: openNetworkSettings ?? () async {},
    refresh: refresh ?? () async => _snapshot,
  );
}

void main() {
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

    expect(find.text('Erledigt'), findsNWidgets(4));
  });
}
