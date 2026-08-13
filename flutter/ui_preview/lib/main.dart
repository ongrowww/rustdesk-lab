import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';

void main() {
  runApp(const OnGrowUiPreviewApp());
}

class OnGrowUiPreviewApp extends StatelessWidget {
  const OnGrowUiPreviewApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'OnGROW Support Desk UI Preview',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: ongrowViolet),
        scaffoldBackgroundColor: const Color(0xFFF6F3FD),
        useMaterial3: true,
      ),
      home: const _PreviewScreen(),
    );
  }
}

class _PreviewScreen extends StatefulWidget {
  const _PreviewScreen();

  @override
  State<_PreviewScreen> createState() => _PreviewScreenState();
}

class _PreviewScreenState extends State<_PreviewScreen> {
  var _snapshot = const OnGrowSupportSnapshot(
    supportId: '682 419 753',
    ready: true,
    canRecordScreen: true,
    isProcessTrusted: false,
    canMonitorInput: false,
    canRecordAudio: false,
    canAcceptIncomingConnections: true,
    version: 'Preview · Mail & Netzwerk',
  );

  Future<OnGrowSupportSnapshot> _refresh() async => _snapshot;

  void _message(String text) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(text)));
  }

  Future<void> _copyId() async {
    await Clipboard.setData(
      ClipboardData(text: _snapshot.supportId.replaceAll(' ', '')),
    );
    _message('Mock-Support-ID kopiert');
  }

  Future<void> _grant({
    bool? screen,
    bool? accessibility,
    bool? inputMonitoring,
    bool? microphone,
  }) async {
    await Future<void>.delayed(const Duration(milliseconds: 250));
    setState(() {
      _snapshot = _snapshot.copyWith(
        canRecordScreen: screen,
        isProcessTrusted: accessibility,
        canMonitorInput: inputMonitoring,
        canRecordAudio: microphone,
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Container(
          constraints: const BoxConstraints(maxWidth: 1120, maxHeight: 620),
          margin: const EdgeInsets.all(24),
          clipBehavior: Clip.antiAlias,
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(20),
            boxShadow: const [
              BoxShadow(
                color: Color(0x2E2E144F),
                blurRadius: 48,
                offset: Offset(0, 24),
              ),
            ],
          ),
          child: OnGrowSupportView(
            snapshot: _snapshot,
            actions: OnGrowSupportActions(
              copySupportId: _copyId,
              requestSupport: () async {
                _message('Preview: E-Mail-App würde geöffnet');
              },
              openSettings: () => _message('Preview: Einstellungen geöffnet'),
              requestScreenRecording: () => _grant(screen: true),
              requestAccessibility: () => _grant(accessibility: true),
              requestInputMonitoring: () => _grant(inputMonitoring: true),
              requestMicrophone: () => _grant(microphone: true),
              openNetworkSettings: () async {
                _message('Preview: Netzwerkeinstellungen würden geöffnet');
              },
              refresh: _refresh,
              enableUnattended: () async {
                setState(() => _snapshot = _snapshot.copyWith(
                      unattendedStatus: OnGrowUnattendedStatus.preparing,
                    ));
                await Future<void>.delayed(const Duration(milliseconds: 600));
                setState(() => _snapshot = _snapshot.copyWith(
                      unattendedStatus: OnGrowUnattendedStatus.enabled,
                    ));
              },
              revokeUnattended: () async {
                setState(() => _snapshot = _snapshot.copyWith(
                      unattendedStatus: OnGrowUnattendedStatus.revoking,
                    ));
                await Future<void>.delayed(const Duration(milliseconds: 600));
                setState(() => _snapshot = _snapshot.copyWith(
                      unattendedStatus: OnGrowUnattendedStatus.notGranted,
                    ));
              },
            ),
          ),
        ),
      ),
    );
  }
}
