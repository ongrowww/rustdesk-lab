import 'package:flutter_hbb/common/ongrow_device_enrollment.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('OnGrowEnrollmentSchedule', () {
    final now = DateTime.utc(2026, 7, 31, 12);

    test('starts only online with a valid ID and blocks parallel work', () {
      final schedule = OnGrowEnrollmentSchedule();

      expect(
        schedule.shouldStart(
          online: false,
          deviceId: 'OG-0001',
          now: now,
        ),
        isFalse,
      );
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: '123456',
          now: now,
        ),
        isFalse,
      );
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now,
        ),
        isTrue,
      );
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now,
        ),
        isFalse,
      );
    });

    test('schedules successful heartbeats every five minutes', () {
      final schedule = OnGrowEnrollmentSchedule();
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now,
        ),
        isTrue,
      );
      schedule.success(now);

      expect(schedule.enrolled, isTrue);
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now.add(const Duration(minutes: 4)),
        ),
        isFalse,
      );
      expect(
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now.add(const Duration(minutes: 5)),
        ),
        isTrue,
      );
    });

    test('backs off, caps retries and resumes immediately', () {
      final schedule = OnGrowEnrollmentSchedule();
      schedule.shouldStart(
        online: true,
        deviceId: 'OG-0001',
        now: now,
      );
      schedule.failure(now, retryable: true, jitter: 0.5);
      expect(schedule.nextSync, now.add(const Duration(seconds: 5)));

      for (var i = 0; i < 20; i++) {
        schedule.resume(now);
        schedule.shouldStart(
          online: true,
          deviceId: 'OG-0001',
          now: now,
        );
        schedule.failure(now, retryable: true, jitter: 1);
      }
      expect(
        schedule.nextSync!.difference(now),
        lessThanOrEqualTo(const Duration(hours: 1)),
      );

      schedule.resume(now);
      expect(schedule.nextSync, now);
    });
  });
}
