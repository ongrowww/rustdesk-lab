class OnGrowControlSyncResult {
  const OnGrowControlSyncResult({
    required this.success,
    required this.enrolled,
    required this.retryable,
    required this.error,
  });

  final bool success;
  final bool enrolled;
  final bool retryable;
  final String error;

  factory OnGrowControlSyncResult.fromJson(Map<String, dynamic> value) {
    return OnGrowControlSyncResult(
      success: value['success'] == true,
      enrolled: value['enrolled'] == true,
      retryable: value['retryable'] == true,
      error: value['error'] is String ? value['error'] as String : '',
    );
  }
}

class OnGrowEnrollmentSchedule {
  OnGrowEnrollmentSchedule({
    this.heartbeatInterval = const Duration(minutes: 5),
    this.initialBackoff = const Duration(seconds: 5),
    this.maximumBackoff = const Duration(hours: 1),
  });

  final Duration heartbeatInterval;
  final Duration initialBackoff;
  final Duration maximumBackoff;

  bool enrolled = false;
  bool running = false;
  DateTime? nextSync;
  String _deviceId = '';
  int _failures = 0;
  bool _disabled = false;

  bool shouldStart({
    required bool online,
    required String deviceId,
    required DateTime now,
  }) {
    if (!online || !_isOnGrowId(deviceId) || running) {
      return false;
    }
    if (_deviceId != deviceId) {
      _deviceId = deviceId;
      enrolled = false;
      _failures = 0;
      _disabled = false;
      nextSync = now;
    }
    if (_disabled) {
      return false;
    }
    if (nextSync != null && now.isBefore(nextSync!)) {
      return false;
    }
    running = true;
    return true;
  }

  void success(DateTime now) {
    running = false;
    enrolled = true;
    _failures = 0;
    nextSync = now.add(heartbeatInterval);
  }

  void failure(
    DateTime now, {
    required bool retryable,
    required double jitter,
    bool reenrollmentRequired = false,
  }) {
    running = false;
    if (reenrollmentRequired) {
      enrolled = false;
    }
    if (!retryable) {
      _disabled = true;
      nextSync = null;
      return;
    }
    final exponent = _failures > 16 ? 16 : _failures;
    final multiplier = 1 << exponent;
    var delay = initialBackoff * multiplier;
    if (delay > maximumBackoff) {
      delay = maximumBackoff;
    }
    final boundedJitter = jitter.clamp(0.0, 1.0);
    final jittered = Duration(
      milliseconds: (delay.inMilliseconds * (0.75 + boundedJitter * 0.5)).round(),
    );
    nextSync = now.add(
      jittered > maximumBackoff ? maximumBackoff : jittered,
    );
    _failures++;
  }

  void resume(DateTime now) {
    if (!running && !_disabled) {
      nextSync = now;
    }
  }

  static bool _isOnGrowId(String value) {
    return RegExp(r'^OG-[0-9]{4}$').hasMatch(value);
  }
}
