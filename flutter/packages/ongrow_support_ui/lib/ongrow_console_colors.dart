import 'package:flutter/material.dart';

/// Paired surfaces and foregrounds for the native operator console.
@immutable
class OnGrowConsoleColors {
  const OnGrowConsoleColors({
    required this.canvas,
    required this.surface,
    required this.inset,
    required this.text,
    required this.muted,
    required this.outline,
    required this.link,
    required this.error,
  });

  static const action = Color(0xFF7516F8);
  static const onAction = Colors.white;
  static const success = Color(0xFFC7FF4A);
  static const onSuccess = Color(0xFF1C1425);

  static const light = OnGrowConsoleColors(
    canvas: Color(0xFFF7F5FA),
    surface: Colors.white,
    inset: Color(0xFFF4F0F8),
    text: Color(0xFF1C1425),
    muted: Color(0xFF61596B),
    outline: Color(0xFF82758F),
    link: action,
    error: Color(0xFFB3261E),
  );

  static const dark = OnGrowConsoleColors(
    canvas: Color(0xFF18151D),
    surface: Color(0xFF24202C),
    inset: Color(0xFF302A39),
    text: Color(0xFFF7F5FA),
    muted: Color(0xFFCCC3D6),
    outline: Color(0xFF9D8FAE),
    link: Color(0xFFC6A5FF),
    error: Color(0xFFFFB4AB),
  );

  static OnGrowConsoleColors forBrightness(Brightness brightness) =>
      brightness == Brightness.dark ? dark : light;

  final Color canvas;
  final Color surface;
  final Color inset;
  final Color text;
  final Color muted;
  final Color outline;
  final Color link;
  final Color error;
}
