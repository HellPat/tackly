import 'dart:ui';

import 'package:flutter/widgets.dart';
import 'package:workmanager/workmanager.dart';

import 'event_store.dart';
import 'family_sync.dart';

const _taskName = 'tacklySync';
const _periodicName = 'tackly-periodic-sync';
const _outboxName = 'tackly-outbox-sync';

@pragma('vm:entry-point')
void callbackDispatcher() {
  Workmanager().executeTask((taskName, inputData) async {
    if (taskName != _taskName) return true;
    WidgetsFlutterBinding.ensureInitialized();
    DartPluginRegistrant.ensureInitialized();
    EventStore? store;
    FamilySync? sync;
    try {
      store = await EventStore.open();
      sync = FamilySync(store);
      await sync.load();
      await sync.syncOnce();
      return true;
    } catch (_) {
      return false;
    } finally {
      sync?.close();
      await store?.close();
    }
  });
}

Future<void> initializeBackgroundSync() =>
    Workmanager().initialize(callbackDispatcher);

Future<void> scheduleBackgroundSync() => Workmanager().registerPeriodicTask(
  _periodicName,
  _taskName,
  frequency: const Duration(minutes: 15),
  constraints: Constraints(networkType: NetworkType.connected),
  existingWorkPolicy: ExistingPeriodicWorkPolicy.keep,
);

Future<void> requestBackgroundSync() => Workmanager().registerOneOffTask(
  _outboxName,
  _taskName,
  constraints: Constraints(networkType: NetworkType.connected),
  existingWorkPolicy: ExistingWorkPolicy.keep,
);
