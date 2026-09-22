import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:ongrow_support_ui/ongrow_console_colors.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';

const fresh = OnGrowSupportSnapshot(
  supportId: 'OG-0000', ready: true, canRecordScreen: false,
  isProcessTrusted: false, canMonitorInput: false,
  canRecordAudio: false, canAcceptIncomingConnections: false,
);

OnGrowSupportActions actions({
  Future<void> Function()? screen,
  Future<void> Function()? accessibility,
  Future<OnGrowSupportSnapshot> Function()? refresh,
  Future<void> Function()? close,
}) => OnGrowSupportActions(
  copySupportId: () async {}, requestSupport: () async {}, openSettings: () {},
  requestScreenRecording: screen ?? () async {},
  requestAccessibility: accessibility ?? () async {},
  requestInputMonitoring: () async {}, requestMicrophone: () async {},
  openNetworkSettings: () async {}, refresh: refresh ?? () async => fresh,
  enableUnattended: () async {}, revokeUnattended: () async {},
  closePermissionGuide: close,
);

void main() {
  void largeScreen(WidgetTester tester) {
    tester.view.physicalSize = const Size(1400, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
  }

  for (final brightness in Brightness.values) {
    testWidgets('$brightness applies to home and every customer dialog', (tester) async {
      largeScreen(tester);
      final palette = OnGrowConsoleColors.forBrightness(brightness);
      await tester.pumpWidget(MaterialApp(
        theme: ThemeData.light(), darkTheme: ThemeData.dark(),
        // Explicit user choice must win, independently of the OS appearance.
        themeMode: brightness == Brightness.dark ? ThemeMode.dark : ThemeMode.light,
        home: Scaffold(body: OnGrowSupportView(snapshot: fresh, actions: actions())),
      ));
      await tester.pumpAndSettle();
      expect(find.byWidgetPredicate((w) => w is ColoredBox && w.color == palette.canvas), findsWidgets);
      expect(find.byWidgetPredicate((w) => w is SvgPicture && w.semanticsLabel == 'OnGROW Support Desk'), findsOneWidget);
      expect(find.text('OnGROW Support Desk'), findsOneWidget); // footer only

      Future<void> surfaceAfterTap(String label) async {
        await tester.ensureVisible(find.text(label));
        await tester.tap(find.text(label));
        await tester.pumpAndSettle();
        expect(find.byWidgetPredicate((w) => w is Container &&
            w.decoration is BoxDecoration &&
            (w.decoration as BoxDecoration).color == palette.surface), findsWidgets);
        expect(tester.takeException(), isNull);
      }
      await surfaceAfterTap('Support anfordern');
      await tester.tap(find.text('Abbrechen'));
      await tester.pumpAndSettle();
      await surfaceAfterTap('Einrichtungshilfe');
      expect(find.text('Bildschirmaufnahme öffnen'), findsOneWidget);
      await tester.tap(find.byTooltip('Schließen'));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('Zugriff für OnGROW freigeben'));
      await tester.tap(find.text('Zugriff für OnGROW freigeben'));
      await tester.pumpAndSettle();
      final dialog = tester.widget<AlertDialog>(find.byType(AlertDialog));
      expect(dialog.backgroundColor, palette.surface);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }

  testWidgets('live theme choice overrides system and can return to system', (tester) async {
    largeScreen(tester);
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);
    final selected = ValueNotifier(ThemeMode.system);
    addTearDown(selected.dispose);
    await tester.pumpWidget(ValueListenableBuilder<ThemeMode>(
      valueListenable: selected,
      builder: (_, mode, __) => MaterialApp(
        theme: ThemeData.light(), darkTheme: ThemeData.dark(), themeMode: mode,
        home: Scaffold(body: OnGrowSupportView(snapshot: fresh, actions: actions())),
      ),
    ));
    Future<void> expectCanvas(Color color) async {
      await tester.pumpAndSettle();
      expect(find.byWidgetPredicate((w) => w is ColoredBox && w.color == color), findsWidgets);
    }
    await expectCanvas(OnGrowConsoleColors.dark.canvas);
    selected.value = ThemeMode.light;
    await expectCanvas(OnGrowConsoleColors.light.canvas);
    selected.value = ThemeMode.dark;
    await expectCanvas(OnGrowConsoleColors.dark.canvas);
    selected.value = ThemeMode.system;
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.light;
    await expectCanvas(OnGrowConsoleColors.light.canvas);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('automatic help opens drag guidance once and follows real grants', (tester) async {
    largeScreen(tester);
    var snapshot = fresh;
    var screenCalls = 0;
    var accessibilityCalls = 0;
    var closed = 0;
    await tester.pumpWidget(MaterialApp(home: OnGrowPermissionHelpDialog(
      initialSnapshot: snapshot, initialStep: 0, autoStart: true,
      actions: actions(
        screen: () async { screenCalls++; },
        accessibility: () async { accessibilityCalls++; },
        refresh: () async => snapshot,
        close: () async { closed++; },
      ),
    )));
    await tester.pumpAndSettle();
    expect(screenCalls, 1);
    expect(accessibilityCalls, 0);
    expect(find.text('Bildschirmaufnahme öffnen'), findsOneWidget);
    snapshot = snapshot.copyWith(canRecordScreen: true);
    await tester.pump(const Duration(seconds: 2));
    await tester.pumpAndSettle();
    expect(find.text('Bedienungshilfen öffnen'), findsOneWidget);
    expect(screenCalls, 1);
    await tester.pumpWidget(const SizedBox());
    expect(closed, 1);
    // Simulate relaunch with actual screen permission now granted.
    await tester.pumpWidget(MaterialApp(home: OnGrowPermissionHelpDialog(
      initialSnapshot: snapshot, initialStep: 1, autoStart: true,
      actions: actions(accessibility: () async { accessibilityCalls++; }),
    )));
    await tester.pumpAndSettle();
    expect(accessibilityCalls, 1);
    expect(screenCalls, 1);
    await tester.pumpWidget(const SizedBox());
  });
}
