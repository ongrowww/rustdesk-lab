import 'dart:math';

import 'package:flutter_hbb/common/ongrow_support_id.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('recognizes only the OnGROW four-digit namespace', () {
    expect(isOnGrowSupportId('OG-0000'), isTrue);
    expect(isOnGrowSupportId('OG-9999'), isTrue);
    expect(isOnGrowSupportId('og-0000'), isFalse);
    expect(isOnGrowSupportId('OG-123'), isFalse);
    expect(isOnGrowSupportId('OG-12345'), isFalse);
    expect(isOnGrowSupportId('OG-12A4'), isFalse);
  });

  test('candidate sequence covers the namespace without duplicates', () {
    final candidates = OnGrowSupportIdCandidates(random: Random(42));
    final generated = <String>{};

    for (var i = 0; i < onGrowSupportIdNamespaceSize; i++) {
      final candidate = candidates.next();
      expect(candidate, isNotNull);
      expect(isOnGrowSupportId(candidate!), isTrue);
      expect(generated.add(candidate), isTrue);
    }

    expect(generated.length, onGrowSupportIdNamespaceSize);
    expect(candidates.next(), isNull);
  });
}
