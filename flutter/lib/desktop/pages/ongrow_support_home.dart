import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/formatter/id_formatter.dart';
import 'package:flutter_hbb/common/ongrow_device_enrollment.dart';
import 'package:flutter_hbb/common/ongrow_support_id.dart';
import 'package:flutter_hbb/desktop/pages/desktop_tab_page.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';
import 'package:url_launcher/url_launcher.dart';

class OnGrowSupportHome extends StatefulWidget {
  const OnGrowSupportHome({super.key});

  @override
  State<OnGrowSupportHome> createState() => _OnGrowSupportHomeState();
}

class _OnGrowSupportHomeState extends State<OnGrowSupportHome>
    with WidgetsBindingObserver {
  static const _maxAutomaticIdAttempts = 64;
  static const _automaticIdResultTimeout = Duration(seconds: 30);

  Timer? _refreshTimer;
  final _controlSchedule = OnGrowEnrollmentSchedule();
  final _random = Random.secure();
  var _controlPlaneConfigured = false;
  var _automaticIdAssignmentRunning = false;
  var _automaticIdAssignmentFinishedForSession = false;
  var _canAcceptIncomingConnections = !isMacOS;
  var _unattendedStatusRefreshAt = DateTime.fromMillisecondsSinceEpoch(0);
  var _unattendedActionRunning = false;
  var _snapshot = const OnGrowSupportSnapshot(
    supportId: '',
    ready: false,
    canRecordScreen: false,
    isProcessTrusted: false,
    canMonitorInput: false,
    canRecordAudio: false,
    canAcceptIncomingConnections: false,
  );

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _controlPlaneConfigured = bind.mainIsOngrowControlConfiguredSync();
    _loadVersion();
    _refresh();
    _refreshNetworkPermission();
    _refreshTimer = Timer.periodic(
      const Duration(seconds: 1),
      (_) => _refresh(),
    );
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _refreshTimer?.cancel();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _controlSchedule.resume(DateTime.now());
      _refresh();
      _refreshNetworkPermission();
    }
  }

  Future<void> _refreshNetworkPermission() async {
    try {
      _canAcceptIncomingConnections =
          !isMacOS || await osxCanAcceptIncomingConnections();
    } catch (_) {
      _canAcceptIncomingConnections = false;
    }
    if (mounted) {
      await _refresh();
    }
  }

  Future<void> _loadVersion() async {
    final version = await bind.mainGetVersion();
    if (mounted) {
      setState(() => _snapshot = _snapshot.copyWith(version: version));
    }
  }

  Future<OnGrowSupportSnapshot> _readSnapshot() async {
    await gFFI.serverModel.fetchID();

    var ready = false;
    try {
      final status =
          jsonDecode(await bind.mainGetConnectStatus()) as Map<String, dynamic>;
      ready = status['status_num'] == 1;
    } catch (_) {
      ready = false;
    }

    var canRecordAudio = false;
    if (isMacOS) {
      canRecordAudio =
          await osxCanRecordAudio() == PermissionAuthorizeType.authorized;
    }

    var unattendedStatus = _snapshot.unattendedStatus;
    var unattendedError = _snapshot.unattendedError;
    final now = DateTime.now();
    if (_controlPlaneConfigured &&
        !_unattendedActionRunning &&
        !now.isBefore(_unattendedStatusRefreshAt)) {
      _unattendedStatusRefreshAt = now.add(const Duration(seconds: 3));
      try {
        final result = _decodeUnattendedResult(
          await bind.mainGetOngrowUnattendedStatus(),
        );
        unattendedStatus = result.status;
        unattendedError = result.error;
      } catch (_) {
        unattendedStatus = OnGrowUnattendedStatus.error;
        unattendedError = 'status_unavailable';
      }
    }

    return OnGrowSupportSnapshot(
      supportId: gFFI.serverModel.serverId.text,
      ready: ready,
      canRecordScreen: !isMacOS || bind.mainIsCanScreenRecording(prompt: false),
      isProcessTrusted: !isMacOS || bind.mainIsProcessTrusted(prompt: false),
      canMonitorInput: !isMacOS || bind.mainIsCanInputMonitoring(prompt: false),
      canRecordAudio: !isMacOS || canRecordAudio,
      canAcceptIncomingConnections: _canAcceptIncomingConnections,
      version: _snapshot.version,
      unattendedStatus: unattendedStatus,
      unattendedError: unattendedError,
    );
  }

  OnGrowUnattendedResult _decodeUnattendedResult(String raw) {
    final decoded = jsonDecode(raw);
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('invalid unattended response');
    }
    return OnGrowUnattendedResult.fromJson(decoded);
  }

  Future<void> _enableUnattended() async {
    if (_unattendedActionRunning) {
      return;
    }
    _unattendedActionRunning = true;
    if (mounted) {
      setState(
        () => _snapshot = _snapshot.copyWith(
          unattendedStatus: OnGrowUnattendedStatus.preparing,
          unattendedError: '',
        ),
      );
    }
    try {
      final result = _decodeUnattendedResult(
        await bind.mainEnableOngrowUnattended(
          screenRecording: _snapshot.canRecordScreen,
          accessibility: _snapshot.isProcessTrusted,
          inputMonitoring: _snapshot.canMonitorInput,
          audioRecording: _snapshot.canRecordAudio,
          network: _snapshot.canAcceptIncomingConnections,
        ),
      );
      if (mounted) {
        setState(
          () => _snapshot = _snapshot.copyWith(
            unattendedStatus: result.status,
            unattendedError: result.error,
          ),
        );
      }
    } catch (_) {
      if (mounted) {
        setState(
          () => _snapshot = _snapshot.copyWith(
            unattendedStatus: OnGrowUnattendedStatus.error,
            unattendedError: 'unattended_failed',
          ),
        );
      }
    } finally {
      _unattendedActionRunning = false;
      _unattendedStatusRefreshAt = DateTime.now().add(
        const Duration(seconds: 2),
      );
    }
  }

  Future<void> _revokeUnattended() async {
    if (_unattendedActionRunning) {
      return;
    }
    _unattendedActionRunning = true;
    if (mounted) {
      setState(
        () => _snapshot = _snapshot.copyWith(
          unattendedStatus: OnGrowUnattendedStatus.revoking,
          unattendedError: '',
        ),
      );
    }
    try {
      final result = _decodeUnattendedResult(
        await bind.mainRevokeOngrowUnattended(),
      );
      if (mounted) {
        setState(
          () => _snapshot = _snapshot.copyWith(
            unattendedStatus: result.status,
            unattendedError: result.error,
          ),
        );
      }
    } catch (_) {
      if (mounted) {
        setState(
          () => _snapshot = _snapshot.copyWith(
            unattendedStatus: OnGrowUnattendedStatus.revoking,
            unattendedError: 'control_plane_unavailable',
          ),
        );
      }
    } finally {
      _unattendedActionRunning = false;
      _unattendedStatusRefreshAt = DateTime.now().add(
        const Duration(seconds: 3),
      );
    }
  }

  Future<OnGrowSupportSnapshot> _refresh() async {
    final next = await _readSnapshot();
    if (mounted) {
      setState(() => _snapshot = next);
    }
    _maybeAssignAutomaticSupportId(next);
    _maybeSyncControlPlane(next);
    return next;
  }

  void _maybeSyncControlPlane(OnGrowSupportSnapshot snapshot) {
    if (!_controlPlaneConfigured) {
      return;
    }
    final id = trimID(snapshot.supportId);
    if (!_controlSchedule.shouldStart(
      online: snapshot.ready,
      deviceId: id,
      now: DateTime.now(),
    )) {
      return;
    }
    unawaited(_syncControlPlane(snapshot));
  }

  Future<void> _syncControlPlane(OnGrowSupportSnapshot snapshot) async {
    try {
      final raw = await bind.mainSyncOngrowControl(
        enrolled: _controlSchedule.enrolled,
        screenRecording: snapshot.canRecordScreen,
        accessibility: snapshot.isProcessTrusted,
        inputMonitoring: snapshot.canMonitorInput,
        audioRecording: snapshot.canRecordAudio,
        network: snapshot.canAcceptIncomingConnections,
      );
      final decoded = jsonDecode(raw);
      if (decoded is! Map<String, dynamic>) {
        throw const FormatException('invalid control response');
      }
      final result = OnGrowControlSyncResult.fromJson(decoded);
      if (result.success && result.enrolled) {
        _controlSchedule.success(DateTime.now());
        return;
      }
      _controlSchedule.failure(
        DateTime.now(),
        retryable: result.retryable,
        jitter: _random.nextDouble(),
        reenrollmentRequired: result.error == 'reenrollment_required',
      );
    } catch (_) {
      _controlSchedule.failure(
        DateTime.now(),
        retryable: true,
        jitter: _random.nextDouble(),
      );
    }
  }

  void _maybeAssignAutomaticSupportId(OnGrowSupportSnapshot snapshot) {
    final currentId = trimID(snapshot.supportId);
    if (isOnGrowSupportId(currentId)) {
      _automaticIdAssignmentFinishedForSession = true;
      return;
    }
    if (!snapshot.ready ||
        currentId.isEmpty ||
        _automaticIdAssignmentRunning ||
        _automaticIdAssignmentFinishedForSession) {
      return;
    }

    _automaticIdAssignmentRunning = true;
    unawaited(_assignAutomaticSupportId());
  }

  Future<void> _assignAutomaticSupportId() async {
    final candidates = OnGrowSupportIdCandidates();
    try {
      for (var attempt = 0; attempt < _maxAutomaticIdAttempts; attempt++) {
        final candidate = candidates.next();
        if (candidate == null) {
          break;
        }

        await bind.mainChangeId(newId: candidate);
        final result = await _waitForAutomaticIdResult();
        if (result == null) {
          debugPrint('Automatic OnGROW support ID assignment timed out');
          return;
        }
        if (result.isEmpty) {
          await _refresh();
          return;
        }
        if (result != 'Not available') {
          debugPrint(
            'Automatic OnGROW support ID assignment failed: $result',
          );
          return;
        }
      }
      debugPrint(
        'Automatic OnGROW support ID assignment exhausted '
        '$_maxAutomaticIdAttempts candidates',
      );
    } finally {
      _automaticIdAssignmentRunning = false;
      _automaticIdAssignmentFinishedForSession = true;
    }
  }

  Future<String?> _waitForAutomaticIdResult() async {
    final deadline = DateTime.now().add(_automaticIdResultTimeout);
    var status = await bind.mainGetAsyncStatus();
    while (status == ' ' && DateTime.now().isBefore(deadline)) {
      await Future<void>.delayed(const Duration(milliseconds: 100));
      status = await bind.mainGetAsyncStatus();
    }
    return status == ' ' ? null : status;
  }

  Future<void> _copySupportId() async {
    final id = trimID(_snapshot.supportId);
    if (id.isEmpty) {
      return;
    }
    await Clipboard.setData(ClipboardData(text: id));
    showToast('Support-ID kopiert');
  }

  Future<void> _requestScreenRecording() async {
    if (isMacOS) {
      bind.mainIsCanScreenRecording(prompt: true);
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  Future<void> _requestAccessibility() async {
    if (isMacOS) {
      bind.mainIsProcessTrusted(prompt: true);
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  Future<void> _requestInputMonitoring() async {
    if (isMacOS) {
      bind.mainIsCanInputMonitoring(prompt: true);
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  Future<void> _requestMicrophone() async {
    if (isMacOS) {
      await osxRequestAudio();
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }

  Future<void> _openNetworkSettings() async {
    if (!isMacOS) {
      return;
    }
    final opened = await launchUrl(
      Uri.parse(
        'x-apple.systempreferences:com.apple.Network-Settings.extension',
      ),
      mode: LaunchMode.externalApplication,
    );
    if (!opened) {
      showToast('Netzwerkeinstellungen konnten nicht geöffnet werden');
    }
  }

  Future<void> _openSupportEmail() async {
    final id = trimID(_snapshot.supportId);
    if (id.isEmpty) {
      return;
    }
    final version = _snapshot.version.isEmpty ? '' : ' ${_snapshot.version}';
    final email = Uri(
      scheme: 'mailto',
      path: 'support@ongrow.de',
      queryParameters: {
        'subject': 'Supportanfrage · OnGROW Support Desk · $id',
        'body': 'Hallo OnGROW Kundensupport,\n\n'
            'ich benötige Unterstützung für dieses Gerät.\n\n'
            'Support-ID: $id\n'
            'App: OnGROW Support Desk$version\n\n'
            'Viele Grüße',
      },
    );
    final opened = await launchUrl(email, mode: LaunchMode.externalApplication);
    if (!opened) {
      showToast('Es konnte keine E-Mail-App geöffnet werden');
    }
  }

  @override
  Widget build(BuildContext context) {
    return OnGrowSupportView(
      snapshot: _snapshot,
      actions: OnGrowSupportActions(
        copySupportId: _copySupportId,
        requestSupport: _openSupportEmail,
        openSettings: DesktopTabPage.onAddSetting,
        requestScreenRecording: _requestScreenRecording,
        requestAccessibility: _requestAccessibility,
        requestInputMonitoring: _requestInputMonitoring,
        requestMicrophone: _requestMicrophone,
        openNetworkSettings: _openNetworkSettings,
        refresh: _refresh,
        enableUnattended: _enableUnattended,
        revokeUnattended: _revokeUnattended,
      ),
    );
  }
}

class OnGrowUnattendedResult {
  const OnGrowUnattendedResult({
    required this.status,
    required this.error,
  });

  factory OnGrowUnattendedResult.fromJson(Map<String, dynamic> json) {
    final rawStatus = json['status'] as String? ?? 'error';
    return OnGrowUnattendedResult(
      status: switch (rawStatus) {
        'not_granted' => OnGrowUnattendedStatus.notGranted,
        'preparing' => OnGrowUnattendedStatus.preparing,
        'enabled' => OnGrowUnattendedStatus.enabled,
        'action_required' => OnGrowUnattendedStatus.actionRequired,
        'revoking' => OnGrowUnattendedStatus.revoking,
        _ => OnGrowUnattendedStatus.error,
      },
      error: json['error'] as String? ?? '',
    );
  }

  final OnGrowUnattendedStatus status;
  final String error;
}
