import 'dart:math';

const onGrowSupportIdNamespaceSize = 10000;

final _onGrowSupportIdPattern = RegExp(r'^OG-\d{4}$');

bool isOnGrowSupportId(String id) => _onGrowSupportIdPattern.hasMatch(id);

class OnGrowSupportIdCandidates {
  OnGrowSupportIdCandidates({Random? random})
      : _random = random ?? Random.secure() {
    _start = _random.nextInt(onGrowSupportIdNamespaceSize);
  }

  final Random _random;
  late final int _start;
  int _offset = 0;
  int? _step;

  String? next() {
    if (_offset >= onGrowSupportIdNamespaceSize) {
      return null;
    }
    final value = (_start + (_offset++ * (_step ??= _nextCoprimeStep()))) %
        onGrowSupportIdNamespaceSize;
    return 'OG-${value.toString().padLeft(4, '0')}';
  }

  int _nextCoprimeStep() {
    int step;
    do {
      step = _random.nextInt(onGrowSupportIdNamespaceSize - 1) + 1;
    } while (step.isEven || step % 5 == 0);
    return step;
  }
}
