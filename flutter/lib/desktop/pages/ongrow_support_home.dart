import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/formatter/id_formatter.dart';
import 'package:flutter_hbb/desktop/pages/desktop_tab_page.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/utils/platform_channel.dart';
import 'package:ongrow_support_ui/ongrow_support_view.dart';

class OnGrowSupportHome extends StatefulWidget {
  const OnGrowSupportHome({super.key});

  @override
  State<OnGrowSupportHome> createState() => _OnGrowSupportHomeState();
}

class _OnGrowSupportHomeState extends State<OnGrowSupportHome> {
  Timer? _refreshTimer;
  var _snapshot = const OnGrowSupportSnapshot(
    supportId: '',
    ready: false,
    canRecordScreen: false,
    isProcessTrusted: false,
    canMonitorInput: false,
    canRecordAudio: false,
  );

  @override
  void initState() {
    super.initState();
    _loadVersion();
    _refresh();
    _refreshTimer = Timer.periodic(
      const Duration(seconds: 1),
      (_) => _refresh(),
    );
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
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

    return OnGrowSupportSnapshot(
      supportId: gFFI.serverModel.serverId.text,
      ready: ready,
      canRecordScreen:
          !isMacOS || bind.mainIsCanScreenRecording(prompt: false),
      isProcessTrusted:
          !isMacOS || bind.mainIsProcessTrusted(prompt: false),
      canMonitorInput:
          !isMacOS || bind.mainIsCanInputMonitoring(prompt: false),
      canRecordAudio: !isMacOS || canRecordAudio,
      version: _snapshot.version,
    );
  }

  Future<OnGrowSupportSnapshot> _refresh() async {
    final next = await _readSnapshot();
    if (mounted) {
      setState(() => _snapshot = next);
    }
    return next;
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

  @override
  Widget build(BuildContext context) {
    return OnGrowSupportView(
      snapshot: _snapshot,
      actions: OnGrowSupportActions(
        copySupportId: _copySupportId,
        requestSupport: _copySupportId,
        openSettings: DesktopTabPage.onAddSetting,
        requestScreenRecording: _requestScreenRecording,
        requestAccessibility: _requestAccessibility,
        requestInputMonitoring: _requestInputMonitoring,
        requestMicrophone: _requestMicrophone,
        refresh: _refresh,
      ),
    );
  }
}
