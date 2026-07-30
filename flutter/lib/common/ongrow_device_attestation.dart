import 'dart:convert';

class OnGrowDeviceAttestationResult {
  const OnGrowDeviceAttestationResult({
    required this.attestation,
    required this.error,
  });

  factory OnGrowDeviceAttestationResult.fromFfi(String raw) {
    try {
      final value = jsonDecode(raw);
      if (value is! Map<String, dynamic>) {
        return const OnGrowDeviceAttestationResult(
          attestation: '',
          error: 'invalid_response',
        );
      }
      final attestation = value['attestation'];
      final error = value['error'];
      if (attestation is! String || error is! String) {
        return const OnGrowDeviceAttestationResult(
          attestation: '',
          error: 'invalid_response',
        );
      }
      return OnGrowDeviceAttestationResult(
        attestation: attestation,
        error: error,
      );
    } on FormatException {
      return const OnGrowDeviceAttestationResult(
        attestation: '',
        error: 'invalid_response',
      );
    }
  }

  final String attestation;
  final String error;

  bool get isSuccess => attestation.isNotEmpty && error.isEmpty;
}
