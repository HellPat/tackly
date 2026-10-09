import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image_picker_platform_interface/image_picker_platform_interface.dart';
import 'package:integration_test/integration_test.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/main.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('smoke: lists, required parent, edit, complete, revert, replay', (
    tester,
  ) async {
    final databaseName = 'smoke_${DateTime.now().microsecondsSinceEpoch}.db';
    final store = await EventStore.open(databaseName: databaseName);
    final controller = AppController(
      store,
      await store.load(),
      captureLocation: () async => const CompletionLocation(
        status: 'captured',
        latitude: 52.52,
        longitude: 13.405,
        accuracyMeters: 4,
      ),
    );
    await tester.pumpWidget(TacklyApp(controller: controller));
    expect(find.text('All Tasks'), findsOneWidget);

    await tester.tap(find.text('All Tasks'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Add task'));
    await tester.pumpAndSettle();
    expect(find.text('Create a list first.'), findsOneWidget);
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();

    await tester.tap(find.text('Add list'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Shopping');
    await tester.tap(find.text('Create list'));
    await tester.pumpAndSettle();
    expect(controller.state.lists.length, 1);
    final listId = controller.state.lists.keys.single;

    await tester.tap(find.text('Add task'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Apples');
    await tester.tap(find.text('Add task').last);
    for (
      var attempt = 0;
      attempt < 30 && controller.state.tasks.isEmpty;
      attempt++
    ) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    await tester.pumpAndSettle();
    final taskId = controller.state.tasks.keys.single;
    expect(controller.state.tasks[taskId]!.listId, listId);
    expect(find.text('Apples'), findsOneWidget);

    await tester.longPress(find.text('Apples'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Green apples');
    await tester.tap(find.text('Save task'));
    await tester.pumpAndSettle();
    expect(find.text('Green apples'), findsOneWidget);

    await tester.tap(find.bySemanticsLabel('Mark Green apples done'));
    await tester.pumpAndSettle();
    expect(controller.state.tasks[taskId]!.isCompleted, isTrue);
    final completion = controller.state.tasks[taskId]!;
    expect(completion.completedAtUtc, isNotNull);
    expect(completion.latitude, 52.52);
    expect(completion.longitude, 13.405);
    expect(completion.locationStatus, 'captured');
    expect(find.text('Green apples'), findsNothing);
    await tester.tap(find.text('↶ Revert'));
    await tester.pumpAndSettle();
    expect(find.text('Green apples'), findsOneWidget);
    expect(controller.state.tasks[taskId]!.isCompleted, isFalse);

    final events = await store.readEvents();
    expect(events.map((e) => e.type).toList(), [
      'list.created',
      'task.created',
      'task.updated',
      'task.completed',
      'task.completion_reverted',
    ]);
    expect(events[3].payload['completedAtUtc'], isNotNull);
    expect(events[3].payload['latitude'], 52.52);
    await store.close();
    final reopened = await EventStore.open(databaseName: databaseName);
    final replayed = await reopened.load();
    expect(replayed.tasks[taskId]!.title, 'Green apples');
    expect(replayed.tasks[taskId]!.isCompleted, isFalse);
    expect((await reopened.readEvents()).length, 5);
    await reopened.close();
  });

  testWidgets(
    'chaos: rapid completion and repeated reversals keep event truth',
    (tester) async {
      final databaseName = 'chaos_${DateTime.now().microsecondsSinceEpoch}.db';
      final store = await EventStore.open(databaseName: databaseName);
      final controller = AppController(
        store,
        await store.load(),
        captureLocation: () async =>
            const CompletionLocation(status: 'unavailable'),
      );
      await tester.pumpWidget(TacklyApp(controller: controller));
      final listId = await controller.createList('Errands', '📍', null);
      await controller.createTask(listId, 'Return bottles', '🍾', null);
      final taskId = controller.state.tasks.keys.single;
      await tester.pumpAndSettle();
      await tester.tap(find.text('Errands'));
      await tester.pumpAndSettle();

      final checkbox = find.bySemanticsLabel('Mark Return bottles done');
      await tester.tap(checkbox);
      await tester.tap(checkbox);
      await tester.pumpAndSettle();
      expect(controller.state.tasks[taskId]!.isCompleted, isTrue);
      expect(controller.state.tasks[taskId]!.completedAtUtc, isNotNull);
      expect(controller.state.tasks[taskId]!.locationStatus, 'unavailable');
      expect(controller.state.tasks[taskId]!.latitude, isNull);
      expect(
        (await store.readEvents())
            .where((e) => e.type == 'task.completed')
            .length,
        1,
      );

      await controller.revertCompletion(
        taskId,
        controller.state.tasks[taskId]!.completionEventId!,
      );
      await tester.pumpAndSettle();
      for (var cycle = 0; cycle < 8; cycle++) {
        final completion = await controller.completeTask(taskId);
        expect(controller.state.tasks[taskId]!.isCompleted, isTrue);
        await controller.revertCompletion(taskId, completion.id);
        await controller.revertCompletion(taskId, completion.id);
        expect(controller.state.tasks[taskId]!.isCompleted, isFalse);
      }
      final events = await store.readEvents();
      expect(events.length, 20); // list + task + 9 completions + 9 reversals
      expect(events.map((e) => e.id).toSet().length, events.length);
      await store.close();
      final reopened = await EventStore.open(databaseName: databaseName);
      final replayed = await reopened.load();
      expect(replayed.tasks[taskId]!.isCompleted, isFalse);
      expect(replayed.tasks[taskId]!.listId, listId);
      expect(replayed.tasks[taskId]!.completedAtUtc, isNull);
      await reopened.close();
    },
  );

  testWidgets('emoji and All Tasks assignment stay with the chosen list', (
    tester,
  ) async {
    final store = await EventStore.open(
      databaseName: 'assignment_${DateTime.now().microsecondsSinceEpoch}.db',
    );
    final controller = AppController(
      store,
      await store.load(),
      captureLocation: () async =>
          const CompletionLocation(status: 'unavailable'),
    );
    await tester.pumpWidget(TacklyApp(controller: controller));
    await tester.tap(find.text('Add list'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('🗂️'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Pick emoji'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextFormField), '🛒');
    await tester.tap(find.text('Use emoji'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Shopping');
    await tester.tap(find.text('Create list'));
    await tester.pumpAndSettle();
    expect(controller.state.lists.values.single.emoji, '🛒');
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();

    await tester.tap(find.text('All Tasks'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Add task'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Milk');
    await tester.tap(find.byType(ChoiceChip).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Add task').last);
    await tester.pumpAndSettle();
    for (
      var attempt = 0;
      attempt < 30 && controller.state.tasks.isEmpty;
      attempt++
    ) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(
      controller.state.tasks.values.single.listId,
      controller.state.lists.keys.single,
    );
    expect(find.text('Milk'), findsOneWidget);
    await tester.longPress(find.text('Milk'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('📝'));
    await tester.pumpAndSettle();
    expect(find.text('Pick emoji'), findsOneWidget);
    expect(find.text('Take photo'), findsOneWidget);
    await tester.tap(find.text('Pick emoji'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextFormField), '🥛');
    await tester.tap(find.text('Use emoji'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Save task'));
    await tester.pumpAndSettle();
    expect(controller.state.tasks.values.single.emoji, '🥛');
    await store.close();
  });

  testWidgets('camera result is saved for a list and task across replay', (
    tester,
  ) async {
    final originalPicker = ImagePickerPlatform.instance;
    final camera = _CameraResult();
    ImagePickerPlatform.instance = camera;
    final databaseName = 'photos_${DateTime.now().microsecondsSinceEpoch}.db';
    final store = await EventStore.open(databaseName: databaseName);
    final controller = AppController(
      store,
      await store.load(),
      captureLocation: () async =>
          const CompletionLocation(status: 'unavailable'),
    );
    try {
      await tester.pumpWidget(TacklyApp(controller: controller));
      await tester.tap(find.text('Add list'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('🗂️'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Take photo'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'Photos');
      await tester.tap(find.text('Create list'));
      await tester.pumpAndSettle();
      for (
        var attempt = 0;
        attempt < 30 && controller.state.lists.isEmpty;
        attempt++
      ) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      final listId = controller.state.lists.keys.single;
      expect(controller.state.lists[listId]!.photo, base64Encode(camera.bytes));

      await tester.tap(find.text('Add task'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('📝'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Take photo'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'Camera task');
      await tester.tap(find.text('Add task').last);
      await tester.pumpAndSettle();
      for (
        var attempt = 0;
        attempt < 30 && controller.state.tasks.isEmpty;
        attempt++
      ) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      final taskId = controller.state.tasks.keys.single;
      expect(controller.state.tasks[taskId]!.photo, base64Encode(camera.bytes));
      expect(camera.calls, 2);
      expect(camera.sources, everyElement(ImageSource.camera));
      await store.close();
      final reopened = await EventStore.open(databaseName: databaseName);
      final replayed = await reopened.load();
      expect(replayed.lists[listId]!.photo, base64Encode(camera.bytes));
      expect(replayed.tasks[taskId]!.photo, base64Encode(camera.bytes));
      await reopened.close();
    } finally {
      ImagePickerPlatform.instance = originalPicker;
      controller.dispose();
    }
  });
}

class _CameraResult extends ImagePickerPlatform {
  final bytes = Uint8List.fromList(
    base64Decode(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC',
    ),
  );
  final sources = <ImageSource>[];
  int calls = 0;

  @override
  Future<XFile?> getImageFromSource({
    required ImageSource source,
    ImagePickerOptions options = const ImagePickerOptions(),
  }) async {
    calls++;
    sources.add(source);
    return XFile.fromData(bytes, mimeType: 'image/png');
  }
}
