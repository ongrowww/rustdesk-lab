import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/ongrow_device_attestation.dart';

void main() {
  group('OnGrowDeviceAttestationResult', () {
    test('accepts a successful FFI response', () {
      final result = OnGrowDeviceAttestationResult.fromFfi(
        '{"attestation":"dGVzdA==","error":""}',
      );

      expect(result.isSuccess, isTrue);
      expect(result.attestation, 'dGVzdA==');
      expect(result.error, isEmpty);
    });

    test('preserves a non-sensitive native error code', () {
      final result = OnGrowDeviceAttestationResult.fromFfi(
        '{"attestation":"","error":"server_not_support"}',
      );

      expect(result.isSuccess, isFalse);
      expect(result.attestation, isEmpty);
      expect(result.error, 'server_not_support');
    });

    test('rejects malformed or incomplete responses', () {
      for (final raw in <String>[
        'not-json',
        '[]',
        '{"attestation":"dGVzdA=="}',
        '{"attestation":7,"error":""}',
      ]) {
        final result = OnGrowDeviceAttestationResult.fromFfi(raw);

        expect(result.isSuccess, isFalse);
        expect(result.attestation, isEmpty);
        expect(result.error, 'invalid_response');
      }
    });
  });
}
