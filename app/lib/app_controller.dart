import 'package:flutter/foundation.dart';
import 'package:geolocator/geolocator.dart';

import 'event_store.dart';

class AppController extends ChangeNotifier {
  AppController(this._store, this.state, {LocationCapture? captureLocation})
    : _locationCapture = captureLocation ?? captureDeviceLocation;

  final EventStore _store;
  final LocationCapture _locationCapture;
  AppState state;

  Future<void> _replay() async {
    state = await _store.load();
    notifyListeners();
  }

  Future<String> createList(String name, String emoji, String? photo) async {
    final id = _store.newId();
    await _store.append('list.created', id, {
      'name': name,
      'emoji': emoji,
      'photo': photo,
    });
    await _replay();
    return id;
  }

  Future<void> createTask(
    String listId,
    String title,
    String emoji,
    String? photo,
  ) async {
    if (!state.lists.containsKey(listId)) {
      throw StateError('A task must belong to a list.');
    }
    final id = _store.newId();
    await _store.append('task.created', id, {
      'listId': listId,
      'title': title,
      'emoji': emoji,
      'photo': photo,
    });
    await _replay();
  }

  Future<void> updateTask(
    String id,
    String title,
    String emoji,
    String? photo,
  ) async {
    final task = state.tasks[id];
    if (task == null || task.isCompleted) {
      throw StateError('This task is no longer open.');
    }
    await _store.append('task.updated', id, {
      'title': title,
      'emoji': emoji,
      'photo': photo,
    });
    await _replay();
  }

  Future<StoredEvent> completeTask(String id) async {
    final task = state.tasks[id];
    if (task == null || task.isCompleted) {
      throw StateError('This task is no longer open.');
    }
    // Capture the tap time before a permission prompt or GPS wait.
    final completedAt = DateTime.now().toUtc();
    final location = await _locationCapture();
    final event = await _store.append('task.completed', id, {
      'completedAtUtc': completedAt.toIso8601String(),
      'latitude': location.latitude,
      'longitude': location.longitude,
      'accuracyMeters': location.accuracyMeters,
      'locationStatus': location.status,
    });
    await _replay();
    return event;
  }

  Future<void> revertCompletion(String taskId, String completionEventId) async {
    final task = state.tasks[taskId];
    if (task?.completionEventId != completionEventId) return;
    await _store.append('task.completion_reverted', taskId, {
      'completionEventId': completionEventId,
    });
    await _replay();
  }
}

typedef LocationCapture = Future<CompletionLocation> Function();

Future<CompletionLocation> captureDeviceLocation() async {
  try {
    if (!await Geolocator.isLocationServiceEnabled()) {
      return const CompletionLocation(status: 'service_disabled');
    }
    var permission = await Geolocator.checkPermission();
    if (permission == LocationPermission.denied) {
      permission = await Geolocator.requestPermission();
    }
    if (permission == LocationPermission.denied ||
        permission == LocationPermission.deniedForever) {
      return const CompletionLocation(status: 'permission_denied');
    }
    // A recent OS fix is enough for the completion tap and avoids making the
    // user wait while the provider seeks another satellite fix.
    final recent = await Geolocator.getLastKnownPosition();
    if (recent != null &&
        DateTime.now().toUtc().difference(recent.timestamp.toUtc()).abs() <
            const Duration(seconds: 30)) {
      return CompletionLocation(
        status: 'captured',
        latitude: recent.latitude,
        longitude: recent.longitude,
        accuracyMeters: recent.accuracy,
      );
    }
    final position = await Geolocator.getCurrentPosition(
      locationSettings: const LocationSettings(
        accuracy: LocationAccuracy.high,
          timeLimit: Duration(seconds: 8),
      ),
    );
    return CompletionLocation(
      status: 'captured',
      latitude: position.latitude,
      longitude: position.longitude,
      accuracyMeters: position.accuracy,
    );
  } catch (error) {
    // A missing fix must not prevent completion. The event records why it has
    // no coordinates, so a future sync can carry exactly what happened.
    if (kDebugMode) debugPrint('Location capture failed: ${error.runtimeType}');
    return const CompletionLocation(status: 'unavailable');
  }
}

class CompletionLocation {
  const CompletionLocation({
    required this.status,
    this.latitude,
    this.longitude,
    this.accuracyMeters,
  });

  final String status;
  final double? latitude;
  final double? longitude;
  final double? accuracyMeters;
}
