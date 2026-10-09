import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/main.dart';

// Run with --dart-define=EXPECT_LOCATION=service_disabled after disabling
// Android location services, or EXPECT_LOCATION=captured after granting
// location permission and setting an emulator GPS fix.
const expectation = String.fromEnvironment('EXPECT_LOCATION');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('Android location result is persisted with completion time', (
    tester,
  ) async {
    final store = await EventStore.open(
      databaseName:
          'device_location_${DateTime.now().microsecondsSinceEpoch}.db',
    );
    final controller = AppController(store, await store.load());
    final listId = await controller.createList('Outside', '📍', null);
    await controller.createTask(listId, 'Walk', '🚶', null);
    final taskId = controller.state.tasks.keys.single;
    await tester.pumpWidget(TacklyApp(controller: controller));
    await tester.tap(find.text('Outside'));
    await tester.pumpAndSettle();
    if (expectation == 'captured') {
      // Allows the emulator harness to grant runtime permission after the
      // Flutter test runner reinstalls the app.
      await Future<void>.delayed(const Duration(seconds: 8));
    }
    await tester.tap(find.bySemanticsLabel('Mark Walk done'));
    await tester.pumpAndSettle();

    final task = controller.state.tasks[taskId]!;
    expect(task.isCompleted, isTrue);
    expect(task.completedAtUtc, isNotNull);
    expect(task.locationStatus, expectation);
    if (expectation == 'captured') {
      expect(task.latitude, isNotNull);
      expect(task.longitude, isNotNull);
      expect(task.accuracyMeters, isNotNull);
    } else {
      expect(task.latitude, isNull);
      expect(task.longitude, isNull);
    }
    final event = (await store.readEvents()).last;
    expect(event.type, 'task.completed');
    expect(event.payload['locationStatus'], expectation);
    await store.close();
  }, skip: expectation.isEmpty);
}
