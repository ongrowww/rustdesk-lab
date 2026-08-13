import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';

void main() {
  testWidgets(
    'shows accessibility and input monitoring as separate permissions',
    (tester) async {
      tester.view.physicalSize = const Size(1400, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      Future<void> noAction() async {}

      final snapshot = OnGrowSupportSnapshot(
        supportId: 'OG-0000',
        ready: true,
        canRecordScreen: false,
        isProcessTrusted: false,
        canMonitorInput: false,
        canRecordAudio: false,
        canAcceptIncomingConnections: false,
      );
      await tester.pumpWidget(
        MaterialApp(
          home: OnGrowSupportView(
            snapshot: snapshot,
            actions: OnGrowSupportActions(
              copySupportId: noAction,
              requestSupport: noAction,
              openSettings: () {},
              requestScreenRecording: noAction,
              requestAccessibility: noAction,
              requestInputMonitoring: noAction,
              requestMicrophone: noAction,
              openNetworkSettings: noAction,
              refresh: () async => snapshot,
              enableUnattended: noAction,
              revokeUnattended: noAction,
            ),
          ),
        ),
      );

      expect(find.text('Bedienungshilfen'), findsOneWidget);
      expect(find.text('Eingabeüberwachung'), findsOneWidget);

      await tester.tap(find.text('Öffnen →').at(2));
      await tester.pumpAndSettle();

      expect(find.text('Eingabeüberwachung öffnen'), findsOneWidget);
      expect(find.text('Bedienungshilfen öffnen'), findsNothing);
    },
  );
}
