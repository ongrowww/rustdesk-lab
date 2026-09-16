import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/utils/multi_window_manager.dart';
import 'package:url_launcher/url_launcher.dart';

class OnGrowSupportConsole extends StatefulWidget {
  const OnGrowSupportConsole({super.key});

  @override
  State<OnGrowSupportConsole> createState() => _OnGrowSupportConsoleState();
}

class _OnGrowSupportConsoleState extends State<OnGrowSupportConsole> {
  static const _purple = Color(0xFF7516F8);
  static const _lime = Color(0xFFC7FF4A);
  Timer? _timer;
  Map<String, dynamic> _status = const {'state': 'loading'};
  bool _opening = false;

  @override
  void initState() {
    super.initState();
    _refresh();
    _timer = Timer.periodic(
      const Duration(milliseconds: 500),
      (_) => _refresh(),
    );
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    try {
      final decoded = jsonDecode(bind.mainGetOngrowOperatorStatusSync());
      if (decoded is Map<String, dynamic> && mounted) {
        setState(() => _status = decoded);
      }
      if (!_opening &&
          decoded is Map<String, dynamic> &&
          decoded['state'] == 'ready') {
        final raw = bind.mainTakeOngrowOperatorLaunchSync();
        if (raw.isNotEmpty) {
          final launch = jsonDecode(raw) as Map<String, dynamic>;
          final deviceId = launch['device_id'];
          final handle = launch['handle'];
          if (deviceId is String &&
              handle is String &&
              deviceId.isNotEmpty &&
              handle.isNotEmpty) {
            _opening = true;
            await rustDeskWinManager.newRemoteDesktop(
              deviceId,
              operatorLaunchHandle: handle,
            );
          }
        }
      }
    } catch (_) {
      if (mounted) {
        setState(
          () => _status = const {
            'state': 'failed',
            'error': 'status_unavailable',
          },
        );
      }
    }
  }

  Future<void> _openControl() async {
    final base = _status['control_plane_url'];
    if (base is! String || base.isEmpty) return;
    await launchUrl(
      Uri.parse('$base/admin/'),
      mode: LaunchMode.externalApplication,
    );
  }

  Future<void> _copy(String key, String label) async {
    final value = _status[key];
    if (value is! String || value.isEmpty) return;
    await Clipboard.setData(ClipboardData(text: value));
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text('$label kopiert.'),
        duration: const Duration(seconds: 2),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final state = _status['state'] as String? ?? 'loading';
    final isRegistered = (_status['console_id'] as String? ?? '').isNotEmpty;
    return Scaffold(
      backgroundColor: const Color(0xFFF7F5FA),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: Padding(
            padding: const EdgeInsets.all(40),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Row(
                  children: [
                    CircleAvatar(
                      radius: 24,
                      backgroundColor: _purple,
                      child: Text(
                        'OG',
                        style: TextStyle(
                          color: Colors.white,
                          fontWeight: FontWeight.w800,
                        ),
                      ),
                    ),
                    SizedBox(width: 16),
                    Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'OnGROW Support Console',
                          style: TextStyle(
                            fontSize: 25,
                            fontWeight: FontWeight.w800,
                          ),
                        ),
                        Text(
                          'Sicherer Zugriff für autorisierte Supportmitarbeiter',
                        ),
                      ],
                    ),
                  ],
                ),
                const SizedBox(height: 32),
                Card(
                  elevation: 0,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(20),
                    side: const BorderSide(color: Color(0xFFE5DDF0)),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.all(28),
                    child: isRegistered
                        ? _registered(state)
                        : _registration(state),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _registration(String state) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Konsole registrieren',
          style: TextStyle(fontSize: 20, fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 8),
        const Text(
          'Öffne Support Control, lege diese Konsole an und kopiere beide öffentlichen Schlüssel in das Formular. Private Schlüssel verlassen den macOS-Schlüsselbund nicht.',
        ),
        const SizedBox(height: 24),
        _keyRow('Signaturschlüssel', 'signing_public_key'),
        const SizedBox(height: 12),
        _keyRow('Verschlüsselungsschlüssel', 'encryption_public_key'),
        const SizedBox(height: 24),
        FilledButton.icon(
          style: FilledButton.styleFrom(
            backgroundColor: _purple,
            foregroundColor: Colors.white,
            minimumSize: const Size.fromHeight(48),
          ),
          onPressed: state == 'failed' ? null : _openControl,
          icon: const Icon(Icons.open_in_new),
          label: const Text('Support Control öffnen'),
        ),
        if (state == 'registering') ...[
          const SizedBox(height: 18),
          const LinearProgressIndicator(color: _purple),
          const SizedBox(height: 8),
          const Text('Registrierung wird kryptografisch bestätigt …'),
        ],
        if (state == 'failed') _error(),
      ],
    );
  }

  Widget _registered(String state) {
    final busy = state == 'redeeming' || state == 'opening';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              decoration: BoxDecoration(
                color: _lime,
                borderRadius: BorderRadius.circular(99),
              ),
              child: const Row(
                children: [
                  Icon(Icons.verified_user_outlined, size: 18),
                  SizedBox(width: 6),
                  Text(
                    'Registriert',
                    style: TextStyle(fontWeight: FontWeight.w700),
                  ),
                ],
              ),
            ),
          ],
        ),
        const SizedBox(height: 20),
        Text(
          busy
              ? 'Supportverbindung wird vorbereitet …'
              : 'Bereit für Supportverbindungen',
          style: const TextStyle(fontSize: 20, fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 8),
        Text(
          busy
              ? 'Das Einmal-Ticket wird geprüft und die Sitzung sicher gestartet.'
              : 'Wähle in Support Control ein freigegebenes Gerät und klicke auf „Supportverbindung starten“.',
        ),
        if (busy) ...[
          const SizedBox(height: 24),
          const LinearProgressIndicator(color: _purple),
        ],
        const SizedBox(height: 24),
        OutlinedButton.icon(
          onPressed: _openControl,
          icon: const Icon(Icons.devices_outlined),
          label: const Text('Geräte in Support Control öffnen'),
        ),
        if (state == 'failed') _error(),
      ],
    );
  }

  Widget _keyRow(String label, String key) {
    final value = _status[key] as String? ?? '';
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: const Color(0xFFF4F0F8),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 4),
                Text(
                  value.isEmpty
                      ? 'Wird erzeugt …'
                      : '${value.substring(0, value.length > 18 ? 18 : value.length)}…',
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
              ],
            ),
          ),
          IconButton(
            tooltip: '$label kopieren',
            onPressed: value.isEmpty ? null : () => _copy(key, label),
            icon: const Icon(Icons.copy_outlined),
          ),
        ],
      ),
    );
  }

  Widget _error() {
    final code = _status['error'] as String? ?? 'unknown_error';
    return Padding(
      padding: const EdgeInsets.only(top: 18),
      child: Text(
        'Die Aktion konnte nicht abgeschlossen werden ($code).',
        style: const TextStyle(color: Colors.redAccent),
      ),
    );
  }
}
