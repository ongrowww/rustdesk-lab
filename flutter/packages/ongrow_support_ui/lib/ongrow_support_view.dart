import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

const ongrowViolet = Color(0xFF7516F8);
const ongrowVioletDark = Color(0xFF381061);
const ongrowInk = Color(0xFF1C1425);
const ongrowMuted = Color(0xFF61596B);
const ongrowCanvas = Color(0xFFFCFCFE);
const ongrowSurface = Color(0xFFF9F8FB);

@immutable
class OnGrowSupportSnapshot {
  const OnGrowSupportSnapshot({
    required this.supportId,
    required this.ready,
    required this.canRecordScreen,
    required this.isProcessTrusted,
    required this.canMonitorInput,
    required this.canRecordAudio,
    required this.canAcceptIncomingConnections,
    this.version = '',
  });

  final String supportId;
  final bool ready;
  final bool canRecordScreen;
  final bool isProcessTrusted;
  final bool canMonitorInput;
  final bool canRecordAudio;
  final bool canAcceptIncomingConnections;
  final String version;

  OnGrowSupportSnapshot copyWith({
    String? supportId,
    bool? ready,
    bool? canRecordScreen,
    bool? isProcessTrusted,
    bool? canMonitorInput,
    bool? canRecordAudio,
    bool? canAcceptIncomingConnections,
    String? version,
  }) {
    return OnGrowSupportSnapshot(
      supportId: supportId ?? this.supportId,
      ready: ready ?? this.ready,
      canRecordScreen: canRecordScreen ?? this.canRecordScreen,
      isProcessTrusted: isProcessTrusted ?? this.isProcessTrusted,
      canMonitorInput: canMonitorInput ?? this.canMonitorInput,
      canRecordAudio: canRecordAudio ?? this.canRecordAudio,
      canAcceptIncomingConnections:
          canAcceptIncomingConnections ?? this.canAcceptIncomingConnections,
      version: version ?? this.version,
    );
  }
}

class OnGrowSupportActions {
  const OnGrowSupportActions({
    required this.copySupportId,
    required this.requestSupport,
    required this.openSettings,
    required this.requestScreenRecording,
    required this.requestAccessibility,
    required this.requestInputMonitoring,
    required this.requestMicrophone,
    required this.openNetworkSettings,
    required this.refresh,
  });

  final Future<void> Function() copySupportId;
  final Future<void> Function() requestSupport;
  final VoidCallback openSettings;
  final Future<void> Function() requestScreenRecording;
  final Future<void> Function() requestAccessibility;
  final Future<void> Function() requestInputMonitoring;
  final Future<void> Function() requestMicrophone;
  final Future<void> Function() openNetworkSettings;
  final Future<OnGrowSupportSnapshot> Function() refresh;
}

class OnGrowSupportView extends StatelessWidget {
  const OnGrowSupportView({
    super.key,
    required this.snapshot,
    required this.actions,
  });

  final OnGrowSupportSnapshot snapshot;
  final OnGrowSupportActions actions;

  Future<void> _showPermissionHelp(
    BuildContext context, {
    int initialStep = 1,
  }) {
    return showDialog<void>(
      context: context,
      barrierColor: const Color(0x7A0E0919),
      builder: (_) => OnGrowPermissionHelpDialog(
        initialSnapshot: snapshot,
        actions: actions,
        initialStep: initialStep,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: ongrowCanvas,
      child: Column(
        children: [
          Expanded(
            child: LayoutBuilder(
              builder: (context, constraints) {
                final compact = constraints.maxWidth < 900;
                final left = _SupportColumn(
                  snapshot: snapshot,
                  actions: actions,
                );
                final right = _PermissionsColumn(
                  snapshot: snapshot,
                  openHelp: (step) =>
                      _showPermissionHelp(context, initialStep: step),
                );
                return SingleChildScrollView(
                  padding: const EdgeInsets.fromLTRB(40, 34, 40, 30),
                  child: compact
                      ? Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            left,
                            const SizedBox(height: 28),
                            right,
                          ],
                        )
                      : Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Expanded(child: left),
                            const SizedBox(width: 44),
                            Expanded(child: right),
                          ],
                        ),
                );
              },
            ),
          ),
          _Footer(version: snapshot.version),
        ],
      ),
    );
  }
}

class _SupportColumn extends StatelessWidget {
  const _SupportColumn({
    required this.snapshot,
    required this.actions,
  });

  final OnGrowSupportSnapshot snapshot;
  final OnGrowSupportActions actions;

  Future<void> _showSupportRequest(BuildContext context) {
    return showDialog<void>(
      context: context,
      barrierColor: const Color(0x940E0919),
      builder: (_) => OnGrowSupportRequestDialog(
        snapshot: snapshot,
        actions: actions,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final id = snapshot.supportId;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            const Expanded(
              child: Text(
                'OnGROW Support Desk',
                style: TextStyle(
                  color: ongrowInk,
                  fontSize: 32,
                  fontWeight: FontWeight.w700,
                  height: 1.2,
                ),
              ),
            ),
            _ReadyPill(ready: snapshot.ready),
          ],
        ),
        const SizedBox(height: 10),
        const Text(
          'Teile deine Support-ID erst, wenn du mit unserem Team sprichst. '
          'Jede Verbindung wird sichtbar angekündigt.',
          style: TextStyle(
            color: Color(0xFF575061),
            fontSize: 15,
            height: 1.45,
          ),
        ),
        const SizedBox(height: 24),
        Container(
          padding: const EdgeInsets.fromLTRB(24, 14, 24, 22),
          decoration: BoxDecoration(
            color: const Color(0xFFF8F4FF),
            borderRadius: BorderRadius.circular(16),
            border: Border.all(color: const Color(0xFFD4C3F0)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  const Expanded(
                    child: Text(
                      'Deine Support-ID',
                      style: TextStyle(
                        color: Color(0xFF2E1C3D),
                        fontSize: 15,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  IconButton(
                    tooltip: 'Einstellungen',
                    onPressed: actions.openSettings,
                    icon: const Icon(Icons.more_vert, size: 20),
                    color: const Color(0xFF6541C7),
                    style: IconButton.styleFrom(
                      backgroundColor: const Color(0xFFF4F0FF),
                      minimumSize: const Size(32, 32),
                      maximumSize: const Size(32, 32),
                      padding: EdgeInsets.zero,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(9),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 10),
              Container(
                height: 70,
                padding: const EdgeInsets.only(left: 16, right: 12),
                decoration: BoxDecoration(
                  color: Colors.white,
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        id.isEmpty ? 'ID wird geladen …' : id,
                        style: const TextStyle(
                          color: ongrowVioletDark,
                          fontSize: 28,
                          fontWeight: FontWeight.w600,
                          letterSpacing: 1.2,
                        ),
                      ),
                    ),
                    IconButton(
                      tooltip: 'Support-ID kopieren',
                      onPressed: id.isEmpty ? null : actions.copySupportId,
                      icon: const Icon(Icons.copy_rounded, size: 18),
                      color: ongrowViolet,
                      style: IconButton.styleFrom(
                        backgroundColor: const Color(0xFFEDE6FB),
                        minimumSize: const Size(38, 38),
                        maximumSize: const Size(38, 38),
                        padding: EdgeInsets.zero,
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(10),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 18),
              SizedBox(
                height: 50,
                child: ElevatedButton(
                  onPressed:
                      id.isEmpty ? null : () => _showSupportRequest(context),
                  style: ElevatedButton.styleFrom(
                    elevation: 0,
                    backgroundColor: ongrowViolet,
                    foregroundColor: Colors.white,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(12),
                    ),
                    padding: const EdgeInsets.symmetric(horizontal: 20),
                  ),
                  child: const Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Support anfordern',
                          style: TextStyle(
                            fontSize: 15,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                      ),
                      Icon(Icons.arrow_forward_rounded, size: 20),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 16),
              const Text(
                'Eine Verbindung beginnt niemals automatisch.',
                style: TextStyle(color: ongrowMuted, fontSize: 12),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _ReadyPill extends StatelessWidget {
  const _ReadyPill({required this.ready});

  final bool ready;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 11, vertical: 7),
      decoration: const ShapeDecoration(
        color: Color(0xFFF0EAF9),
        shape: StadiumBorder(),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 7,
            height: 7,
            decoration: BoxDecoration(
              color: ready ? const Color(0xFF9FCB31) : const Color(0xFFE05B68),
              shape: BoxShape.circle,
            ),
          ),
          const SizedBox(width: 7),
          Text(
            ready ? 'Bereit für Support' : 'Verbindung wird geprüft',
            style: const TextStyle(
              color: Color(0xFF514A57),
              fontSize: 11,
              fontWeight: FontWeight.w600,
            ),
          ),
        ],
      ),
    );
  }
}

class _PermissionsColumn extends StatelessWidget {
  const _PermissionsColumn({
    required this.snapshot,
    required this.openHelp,
  });

  final OnGrowSupportSnapshot snapshot;
  final void Function(int step) openHelp;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            const Expanded(
              child: Text(
                'Mac-Berechtigungen',
                style: TextStyle(
                  color: ongrowInk,
                  fontSize: 22,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            TextButton.icon(
              onPressed: () => openHelp(1),
              icon: const Icon(Icons.help_outline_rounded, size: 14),
              label: const Text('Einrichtungshilfe'),
              style: TextButton.styleFrom(
                foregroundColor: const Color(0xFF334F14),
                backgroundColor: const Color(0xFFE8F9BE),
                textStyle: const TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                ),
                minimumSize: const Size(0, 28),
                padding: const EdgeInsets.symmetric(horizontal: 10),
                shape: const StadiumBorder(),
              ),
            ),
          ],
        ),
        const SizedBox(height: 4),
        const Text(
          'Damit wir sehen und helfen können – du behältst die Kontrolle.',
          style: TextStyle(color: ongrowMuted, fontSize: 13),
        ),
        const SizedBox(height: 20),
        Container(
          padding: const EdgeInsets.all(18),
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(16),
            border: Border.all(color: const Color(0xFFE3DFE9)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Text(
                'Vor dem ersten Support',
                style: TextStyle(
                  color: Color(0xFF40304A),
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 10),
              _PermissionOverviewRow(
                icon: Icons.desktop_mac_outlined,
                iconColor: const Color(0xFF719000),
                title: 'Bildschirmaufnahme',
                detail: 'Ermöglicht die Bildschirmansicht',
                granted: snapshot.canRecordScreen,
                onPressed: snapshot.canRecordScreen ? null : () => openHelp(0),
              ),
              const SizedBox(height: 10),
              _PermissionOverviewRow(
                icon: Icons.accessibility_new_rounded,
                iconColor: ongrowViolet,
                title: 'Bedienungshilfen',
                detail: 'Erlaubt die Fernsteuerung von Maus und Tastatur',
                granted: snapshot.isProcessTrusted,
                onPressed:
                    snapshot.isProcessTrusted ? null : () => openHelp(1),
              ),
              const SizedBox(height: 10),
              _PermissionOverviewRow(
                icon: Icons.keyboard_alt_outlined,
                iconColor: const Color(0xFF5A4B6C),
                title: 'Eingabeüberwachung',
                detail: 'Erkennt lokale Eingaben während des Supports',
                granted: snapshot.canMonitorInput,
                onPressed:
                    snapshot.canMonitorInput ? null : () => openHelp(2),
              ),
              const SizedBox(height: 10),
              _PermissionOverviewRow(
                icon: Icons.router_outlined,
                iconColor: const Color(0xFF6541C7),
                title: 'Netzwerkzugriff',
                detail: 'Erlaubt eingehende Supportverbindungen',
                granted: snapshot.canAcceptIncomingConnections,
                statusLabel:
                    snapshot.canAcceptIncomingConnections ? null : 'Prüfen →',
                onPressed: snapshot.canAcceptIncomingConnections
                    ? null
                    : () => openHelp(3),
              ),
              const SizedBox(height: 10),
              _PermissionOverviewRow(
                icon: Icons.mic_none_rounded,
                iconColor: const Color(0xFF81798A),
                title: 'Mikrofon',
                detail: 'Nur für Sprachübertragung erforderlich',
                granted: snapshot.canRecordAudio,
                optional: true,
                onPressed: () => openHelp(4),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _PermissionOverviewRow extends StatelessWidget {
  const _PermissionOverviewRow({
    required this.icon,
    required this.iconColor,
    required this.title,
    required this.detail,
    required this.granted,
    this.optional = false,
    this.statusLabel,
    this.onPressed,
  });

  final IconData icon;
  final Color iconColor;
  final String title;
  final String detail;
  final bool granted;
  final bool optional;
  final String? statusLabel;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 72,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      decoration: BoxDecoration(
        color: ongrowSurface,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          Icon(icon, color: iconColor, size: 28),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: const TextStyle(
                    color: Color(0xFF261F2E),
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 3),
                Text(
                  detail,
                  style: const TextStyle(
                    color: Color(0xFF6E6675),
                    fontSize: 11,
                  ),
                ),
              ],
            ),
          ),
          if (granted)
            const _StatusPill(
              label: '✓ Erlaubt',
              background: Color(0xFFE8F9BE),
              foreground: Color(0xFF4F6400),
            )
          else if (optional)
            _StatusPill(
              label: statusLabel ?? 'Optional',
              background: const Color(0xFFEDEBF0),
              foreground: const Color(0xFF5C5463),
              onPressed: onPressed,
            )
          else
            _StatusPill(
              label: statusLabel ?? 'Öffnen →',
              background: const Color(0xFFEDE6FB),
              foreground: ongrowViolet,
              onPressed: onPressed,
            ),
        ],
      ),
    );
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({
    required this.label,
    required this.background,
    required this.foreground,
    this.onPressed,
  });

  final String label;
  final Color background;
  final Color foreground;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final content = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
      child: Text(
        label,
        style: TextStyle(
          color: foreground,
          fontSize: 11,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
    if (onPressed == null) {
      return DecoratedBox(
        decoration: ShapeDecoration(
          color: background,
          shape: const StadiumBorder(),
        ),
        child: content,
      );
    }
    return Material(
      color: background,
      shape: const StadiumBorder(),
      child: InkWell(
        customBorder: const StadiumBorder(),
        onTap: onPressed,
        child: content,
      ),
    );
  }
}

class _Footer extends StatelessWidget {
  const _Footer({required this.version});

  final String version;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 56,
      color: const Color(0xFFF6F5F9),
      padding: const EdgeInsets.symmetric(horizontal: 40),
      child: Row(
        children: [
          const Icon(
            Icons.lock_outline_rounded,
            size: 14,
            color: ongrowViolet,
          ),
          const SizedBox(width: 8),
          const Expanded(
            child: Text(
              'Zugriff nur nach deiner ausdrücklichen Freigabe',
              style: TextStyle(color: Color(0xFF595261), fontSize: 11),
            ),
          ),
          SvgPicture.asset(
            'assets/ongrow-logo.svg',
            width: 98,
            height: 16,
            package: 'ongrow_support_ui',
          ),
          Expanded(
            child: Align(
              alignment: Alignment.centerRight,
              child: Text(
                'OnGROW Support Desk${version.isEmpty ? '' : ' · $version'}',
                style: const TextStyle(
                  color: Color(0xFF736B7A),
                  fontSize: 11,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class OnGrowSupportRequestDialog extends StatefulWidget {
  const OnGrowSupportRequestDialog({
    super.key,
    required this.snapshot,
    required this.actions,
  });

  final OnGrowSupportSnapshot snapshot;
  final OnGrowSupportActions actions;

  @override
  State<OnGrowSupportRequestDialog> createState() =>
      _OnGrowSupportRequestDialogState();
}

class _OnGrowSupportRequestDialogState
    extends State<OnGrowSupportRequestDialog> {
  bool _openingEmail = false;

  Future<void> _openEmail() async {
    if (_openingEmail) {
      return;
    }
    setState(() => _openingEmail = true);
    try {
      await widget.actions.requestSupport();
      if (mounted) {
        Navigator.of(context).pop();
      }
    } finally {
      if (mounted) {
        setState(() => _openingEmail = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final version = widget.snapshot.version;
    return Dialog(
      backgroundColor: Colors.transparent,
      insetPadding: const EdgeInsets.all(24),
      child: Container(
        width: 620,
        padding: const EdgeInsets.fromLTRB(24, 24, 24, 22),
        decoration: BoxDecoration(
          color: Colors.white,
          borderRadius: BorderRadius.circular(18),
          boxShadow: const [
            BoxShadow(
              color: Color(0x3D140A26),
              blurRadius: 42,
              offset: Offset(0, 18),
            ),
          ],
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Support anfordern',
                        style: TextStyle(
                          color: ongrowInk,
                          fontSize: 22,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      SizedBox(height: 7),
                      Text(
                        'Wähle deinen Supportkontakt. Wir bereiten '
                        'anschließend eine E-Mail mit deiner Support-ID vor.',
                        style: TextStyle(
                          color: Color(0xFF61596B),
                          fontSize: 12,
                          height: 1.4,
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 18),
                IconButton(
                  tooltip: 'Schließen',
                  onPressed: () => Navigator.of(context).pop(),
                  icon: const Icon(Icons.close_rounded, size: 20),
                  style: IconButton.styleFrom(
                    backgroundColor: const Color(0xFFF5F3F7),
                    minimumSize: const Size(34, 34),
                    maximumSize: const Size(34, 34),
                    padding: EdgeInsets.zero,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(10),
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            const Text(
              'Supportkontakt',
              style: TextStyle(
                color: Color(0xFF40304A),
                fontSize: 12,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 8),
            Container(
              height: 78,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              decoration: BoxDecoration(
                color: const Color(0xFFF8F4FF),
                borderRadius: BorderRadius.circular(13),
                border: Border.all(color: ongrowViolet, width: 1.5),
              ),
              child: const Row(
                children: [
                  _SupportAvatar(),
                  SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'OnGROW GmbH Kundensupport',
                          style: TextStyle(
                            color: ongrowInk,
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        SizedBox(height: 4),
                        Text(
                          'support@ongrow.de',
                          style: TextStyle(
                            color: ongrowMuted,
                            fontSize: 12,
                          ),
                        ),
                      ],
                    ),
                  ),
                  Icon(
                    Icons.check_circle_rounded,
                    color: ongrowViolet,
                    size: 22,
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            Container(
              height: 64,
              padding: const EdgeInsets.symmetric(horizontal: 14),
              decoration: BoxDecoration(
                color: ongrowSurface,
                borderRadius: BorderRadius.circular(12),
              ),
              child: Row(
                children: [
                  const Icon(
                    Icons.mail_outline_rounded,
                    color: ongrowViolet,
                    size: 20,
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Text(
                          'Wird in die E-Mail übernommen',
                          style: TextStyle(
                            color: Color(0xFF40304A),
                            fontSize: 11,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          'Support-ID ${widget.snapshot.supportId}'
                          '${version.isEmpty ? '' : '  ·  '
                              'OnGROW Support Desk $version'}',
                          style: const TextStyle(
                            color: ongrowMuted,
                            fontSize: 12,
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            const Row(
              children: [
                Icon(
                  Icons.lock_outline_rounded,
                  color: ongrowMuted,
                  size: 14,
                ),
                SizedBox(width: 7),
                Expanded(
                  child: Text(
                    'Kein Passwort wird übermittelt. '
                    'Du sendest die E-Mail selbst.',
                    style: TextStyle(color: ongrowMuted, fontSize: 11),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(
                  onPressed:
                      _openingEmail ? null : () => Navigator.of(context).pop(),
                  style: TextButton.styleFrom(
                    foregroundColor: const Color(0xFF40304A),
                    backgroundColor: const Color(0xFFF3F0F6),
                    minimumSize: const Size(96, 44),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(11),
                    ),
                  ),
                  child: const Text('Abbrechen'),
                ),
                const SizedBox(width: 10),
                ElevatedButton.icon(
                  onPressed: _openingEmail ? null : _openEmail,
                  icon: _openingEmail
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: Colors.white,
                          ),
                        )
                      : const Icon(Icons.mail_outline_rounded, size: 17),
                  label: const Text('E-Mail-App öffnen'),
                  style: ElevatedButton.styleFrom(
                    elevation: 0,
                    foregroundColor: Colors.white,
                    backgroundColor: ongrowViolet,
                    disabledForegroundColor: Colors.white70,
                    disabledBackgroundColor: const Color(0xFF9D76D3),
                    minimumSize: const Size(190, 44),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(11),
                    ),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _SupportAvatar extends StatelessWidget {
  const _SupportAvatar();

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 46,
      height: 46,
      alignment: Alignment.center,
      decoration: const BoxDecoration(
        color: Color(0xFFC9FF4A),
        shape: BoxShape.circle,
      ),
      child: const Text(
        'OG',
        style: TextStyle(
          color: ongrowInk,
          fontSize: 13,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

class OnGrowPermissionHelpDialog extends StatefulWidget {
  const OnGrowPermissionHelpDialog({
    super.key,
    required this.initialSnapshot,
    required this.actions,
    required this.initialStep,
  });

  final OnGrowSupportSnapshot initialSnapshot;
  final OnGrowSupportActions actions;
  final int initialStep;

  @override
  State<OnGrowPermissionHelpDialog> createState() =>
      _OnGrowPermissionHelpDialogState();
}

class _OnGrowPermissionHelpDialogState
    extends State<OnGrowPermissionHelpDialog> {
  late OnGrowSupportSnapshot _snapshot;
  late int _expandedStep;
  Timer? _refreshTimer;
  bool _busy = false;
  bool _refreshing = false;

  @override
  void initState() {
    super.initState();
    _snapshot = widget.initialSnapshot;
    _expandedStep = widget.initialStep;
    _refreshTimer = Timer.periodic(
      const Duration(seconds: 1),
      (_) => _refreshSnapshot(),
    );
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  Future<void> _refreshSnapshot() async {
    if (_busy || _refreshing) {
      return;
    }
    _refreshing = true;
    try {
      final refreshed = await widget.actions.refresh();
      if (mounted) {
        setState(() => _snapshot = refreshed);
      }
    } finally {
      _refreshing = false;
    }
  }

  Future<void> _run(Future<void> Function() action) async {
    if (_busy) {
      return;
    }
    setState(() => _busy = true);
    try {
      await action();
      final refreshed = await widget.actions.refresh();
      if (mounted) {
        setState(() => _snapshot = refreshed);
      }
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: Colors.transparent,
      insetPadding: const EdgeInsets.all(24),
      child: Container(
        width: 640,
        constraints: const BoxConstraints(maxHeight: 560),
        padding: const EdgeInsets.fromLTRB(24, 22, 24, 20),
        decoration: BoxDecoration(
          color: Colors.white,
          borderRadius: BorderRadius.circular(18),
          boxShadow: const [
            BoxShadow(
              color: Color(0x3D140A26),
              blurRadius: 42,
              offset: Offset(0, 18),
            ),
          ],
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _DialogHeader(onClose: () => Navigator.of(context).pop()),
            const SizedBox(height: 16),
            Flexible(
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    _HelpStep(
                      title: 'Bildschirmaufnahme',
                      icon: Icons.desktop_mac_outlined,
                      complete: _snapshot.canRecordScreen,
                      expanded: _expandedStep == 0,
                      onToggle: () => setState(() => _expandedStep = 0),
                      child: _StepInstructions(
                        instructions: const [
                          'Systemeinstellungen öffnen.',
                          'Datenschutz & Sicherheit → Bildschirmaufnahme wählen.',
                          'OnGROW Support Desk aktivieren.',
                        ],
                        buttonLabel: 'Bildschirmaufnahme öffnen',
                        busy: _busy,
                        onPressed: () =>
                            _run(widget.actions.requestScreenRecording),
                      ),
                    ),
                    const SizedBox(height: 10),
                    _HelpStep(
                      title: 'Bedienungshilfen',
                      icon: Icons.accessibility_new_rounded,
                      complete: _snapshot.isProcessTrusted,
                      expanded: _expandedStep == 1,
                      onToggle: () => setState(() => _expandedStep = 1),
                      child: _StepInstructions(
                        instructions: const [
                          'Systemeinstellungen öffnen.',
                          'Datenschutz & Sicherheit → Bedienungshilfen wählen.',
                          'OnGROW Support Desk aktivieren.',
                        ],
                        buttonLabel: 'Bedienungshilfen öffnen',
                        busy: _busy,
                        onPressed: () =>
                            _run(widget.actions.requestAccessibility),
                      ),
                    ),
                    const SizedBox(height: 10),
                    _HelpStep(
                      title: 'Eingabeüberwachung',
                      icon: Icons.keyboard_alt_outlined,
                      complete: _snapshot.canMonitorInput,
                      expanded: _expandedStep == 2,
                      onToggle: () => setState(() => _expandedStep = 2),
                      child: _StepInstructions(
                        instructions: const [
                          'Systemeinstellungen öffnen.',
                          'Datenschutz & Sicherheit → Eingabeüberwachung wählen.',
                          'OnGROW Support Desk aktivieren.',
                        ],
                        buttonLabel: 'Eingabeüberwachung öffnen',
                        busy: _busy,
                        onPressed: () =>
                            _run(widget.actions.requestInputMonitoring),
                      ),
                    ),
                    const SizedBox(height: 10),
                    _HelpStep(
                      title: 'Netzwerkzugriff',
                      icon: Icons.router_outlined,
                      complete: _snapshot.canAcceptIncomingConnections,
                      statusLabel: _snapshot.canAcceptIncomingConnections
                          ? null
                          : 'Bei Nachfrage',
                      expanded: _expandedStep == 3,
                      onToggle: () => setState(() => _expandedStep = 3),
                      child: _StepInstructions(
                        instructions: const [
                          'Erlaube eingehende Netzwerkverbindungen, '
                              'wenn macOS danach fragt.',
                          'Prüfe bei Bedarf unter Netzwerk → Firewall → '
                              'Optionen, dass OnGROW Support Desk '
                              'Verbindungen annehmen darf.',
                        ],
                        buttonLabel: 'Netzwerkeinstellungen öffnen',
                        busy: _busy,
                        onPressed: () =>
                            _run(widget.actions.openNetworkSettings),
                      ),
                    ),
                    const SizedBox(height: 10),
                    _HelpStep(
                      title: 'Mikrofon',
                      icon: Icons.mic_none_rounded,
                      complete: _snapshot.canRecordAudio,
                      optional: true,
                      expanded: _expandedStep == 4,
                      onToggle: () => setState(() => _expandedStep = 4),
                      child: _StepInstructions(
                        instructions: const [
                          'Diese Freigabe ist nur für Sprachübertragung nötig.',
                          'Bestätige den nativen macOS-Dialog.',
                        ],
                        buttonLabel: 'Mikrofonfreigabe anfragen',
                        busy: _busy,
                        onPressed: () => _run(widget.actions.requestMicrophone),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 14),
            const Row(
              children: [
                Icon(
                  Icons.lock_outline_rounded,
                  size: 16,
                  color: Color(0xFF6B6470),
                ),
                SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'Die Freigabe erfolgt immer direkt durch dich in macOS.',
                    style: TextStyle(color: Color(0xFF6B6470), fontSize: 11),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _DialogHeader extends StatelessWidget {
  const _DialogHeader({required this.onClose});

  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Mac-Berechtigungen einrichten',
                style: TextStyle(
                  color: ongrowInk,
                  fontSize: 20,
                  fontWeight: FontWeight.w600,
                ),
              ),
              SizedBox(height: 5),
              Text(
                'Öffne die Schritte nacheinander und erteile die '
                'Freigaben direkt in macOS.',
                style: TextStyle(color: Color(0xFF645E69), fontSize: 13),
              ),
            ],
          ),
        ),
        IconButton(
          tooltip: 'Schließen',
          onPressed: onClose,
          icon: const Icon(Icons.close_rounded, size: 20),
          style: IconButton.styleFrom(
            backgroundColor: const Color(0xFFF5F3F7),
            minimumSize: const Size(34, 34),
            maximumSize: const Size(34, 34),
            padding: EdgeInsets.zero,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(10),
            ),
          ),
        ),
      ],
    );
  }
}

class _HelpStep extends StatelessWidget {
  const _HelpStep({
    required this.title,
    required this.icon,
    required this.complete,
    required this.expanded,
    required this.onToggle,
    required this.child,
    this.optional = false,
    this.statusLabel,
  });

  final String title;
  final IconData icon;
  final bool complete;
  final bool expanded;
  final VoidCallback onToggle;
  final Widget child;
  final bool optional;
  final String? statusLabel;

  @override
  Widget build(BuildContext context) {
    return AnimatedContainer(
      duration: const Duration(milliseconds: 160),
      decoration: BoxDecoration(
        color: expanded ? const Color(0xFFF8F4FF) : ongrowSurface,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: expanded ? ongrowViolet : const Color(0xFFE3DFE9),
          width: expanded ? 1.5 : 1,
        ),
      ),
      child: Column(
        children: [
          InkWell(
            onTap: onToggle,
            borderRadius: BorderRadius.circular(12),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 13),
              child: Row(
                children: [
                  Container(
                    width: 24,
                    height: 24,
                    decoration: BoxDecoration(
                      color: complete
                          ? const Color(0xFFD9FAA7)
                          : expanded
                              ? ongrowViolet
                              : const Color(0xFFEDEBF0),
                      shape: BoxShape.circle,
                    ),
                    child: Icon(
                      complete ? Icons.check_rounded : icon,
                      size: 16,
                      color: complete
                          ? const Color(0xFF335B12)
                          : expanded
                              ? Colors.white
                              : const Color(0xFF665F6D),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      title,
                      style: const TextStyle(
                        color: Color(0xFF261F2E),
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  _StatusPill(
                    label: complete
                        ? 'Erledigt'
                        : statusLabel != null
                            ? statusLabel!
                            : optional
                                ? 'Optional'
                                : 'Jetzt einrichten',
                    background: complete
                        ? const Color(0xFFE8F9BE)
                        : const Color(0xFFF0ECF8),
                    foreground:
                        complete ? const Color(0xFF4F6400) : ongrowViolet,
                  ),
                  const SizedBox(width: 8),
                  Icon(
                    expanded
                        ? Icons.keyboard_arrow_up_rounded
                        : Icons.keyboard_arrow_down_rounded,
                    size: 20,
                    color: const Color(0xFF665F6D),
                  ),
                ],
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(48, 0, 16, 16),
              child: child,
            ),
        ],
      ),
    );
  }
}

class _StepInstructions extends StatelessWidget {
  const _StepInstructions({
    required this.instructions,
    required this.buttonLabel,
    required this.busy,
    required this.onPressed,
  });

  final List<String> instructions;
  final String buttonLabel;
  final bool busy;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (var index = 0; index < instructions.length; index++)
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Text(
              '${index + 1}. ${instructions[index]}',
              style: const TextStyle(
                color: Color(0xFF514A57),
                fontSize: 12,
                height: 1.35,
              ),
            ),
          ),
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: _SettingsAction(
            label: buttonLabel,
            busy: busy,
            onPressed: onPressed,
          ),
        ),
      ],
    );
  }
}

class _SettingsAction extends StatelessWidget {
  const _SettingsAction({
    required this.label,
    required this.busy,
    required this.onPressed,
  });

  final String label;
  final bool busy;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 38,
      child: OutlinedButton.icon(
        onPressed: busy ? null : onPressed,
        icon: const Icon(Icons.settings_outlined, size: 16),
        label: Text(label),
        style: OutlinedButton.styleFrom(
          foregroundColor: Colors.white,
          backgroundColor: ongrowViolet,
          disabledForegroundColor: Colors.white70,
          disabledBackgroundColor: const Color(0xFF9D76D3),
          side: const BorderSide(color: ongrowViolet),
          textStyle: const TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w600,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(9),
          ),
        ),
      ),
    );
  }
}
