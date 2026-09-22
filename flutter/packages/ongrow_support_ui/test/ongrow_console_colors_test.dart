import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ongrow_support_ui/ongrow_console_colors.dart';

double contrast(Color foreground, Color background) {
  final a = foreground.computeLuminance();
  final b = background.computeLuminance();
  return ((a > b ? a : b) + 0.05) / ((a > b ? b : a) + 0.05);
}

void main() {
  for (final brightness in Brightness.values) {
    test('$brightness keeps console text and controls legible', () {
      final colors = OnGrowConsoleColors.forBrightness(brightness);
      for (final background in [colors.canvas, colors.surface, colors.inset]) {
        for (final foreground in [colors.text, colors.muted]) {
          expect(contrast(foreground, background), greaterThanOrEqualTo(4.5));
        }
      }
      expect(contrast(colors.link, colors.surface), greaterThanOrEqualTo(4.5));
      expect(contrast(colors.error, colors.surface), greaterThanOrEqualTo(4.5));
      expect(contrast(colors.outline, colors.surface), greaterThanOrEqualTo(3));
    });
  }
  test('brand button and registration badge have contrasting labels', () {
    expect(
      contrast(OnGrowConsoleColors.onAction, OnGrowConsoleColors.action),
      greaterThanOrEqualTo(4.5),
    );
    expect(
      contrast(OnGrowConsoleColors.onSuccess, OnGrowConsoleColors.success),
      greaterThanOrEqualTo(4.5),
    );
  });
}
