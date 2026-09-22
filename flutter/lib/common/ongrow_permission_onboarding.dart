/// Persist only workflow progress, never cached macOS authorization decisions.
/// A system-requested relaunch resumes at the first permission still missing.
class OnGrowPermissionOnboarding {
  static const optionKey = 'ongrow-permission-onboarding-v1';
  static const complete = 'complete';

  static int? nextStep({
    required bool screen,
    required bool accessibility,
    required bool input,
  }) {
    if (!screen) return 0;
    if (!accessibility) return 1;
    if (!input) return 2;
    return null;
  }

  static int? startupStep({
    required String saved,
    required bool screen,
    required bool accessibility,
    required bool input,
  }) => saved == complete ? null : nextStep(
    screen: screen, accessibility: accessibility, input: input);
}
