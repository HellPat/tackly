import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:geolocator/geolocator.dart';

import 'event_store.dart';
import 'background_sync.dart';
import 'family_sync.dart';

class AppController extends ChangeNotifier {
  AppController(this._store, this.state, {LocationCapture? captureLocation})
    : _locationCapture = captureLocation ?? captureDeviceLocation;

  final EventStore _store;
  final LocationCapture _locationCapture;
  AppState state;
  FamilySync? familySync;
  Timer? _syncTimer;
  bool _syncing = false;
  bool _loggingOut = false;
  String? syncError;

  void attachFamilySync(FamilySync service) {
    familySync = service;
    _syncTimer?.cancel();
    _syncTimer = Timer.periodic(
      const Duration(seconds: 20),
      (_) => unawaited(syncNow()),
    );
    unawaited(syncNow());
    notifyListeners();
  }

  Future<void> syncNow() async {
    final service = familySync;
    if (service?.credentials == null || _syncing || _loggingOut) return;
    _syncing = true;
    try {
      final changed = await service!.syncOnce();
      syncError = null;
      if (changed) await _replay();
    } on SyncHttpException catch (error) {
      syncError = error.statusCode == 401
          ? 'Sync access expired · local changes stay on this phone'
          : 'Sync failed (${error.statusCode}) · local changes stay on this phone';
    } catch (_) {
      syncError = 'Sync failed · local changes stay on this phone';
    } finally {
      _syncing = false;
      notifyListeners();
    }
  }

  @override
  void dispose() {
    _syncTimer?.cancel();
    familySync?.close();
    super.dispose();
  }

  Future<void> _replay({bool localChange = false}) async {
    state = await _store.load();
    notifyListeners();
    if (localChange) unawaited(syncNow());
    if (localChange && familySync?.credentials != null) {
      unawaited(requestBackgroundSync().catchError((Object _) {}));
    }
  }

  Future<void> refreshFromDisk() => _replay();

  Future<int> pendingChangeCount() async =>
      (await _store.pendingEvents()).length;

  Future<void> logout() async {
    final service = familySync;
    if (service?.credentials == null) return;
    _loggingOut = true;
    try {
      while (_syncing) {
        await Future<void>.delayed(const Duration(milliseconds: 20));
      }
      try {
        await cancelBackgroundSync();
      } catch (_) {
        // Foreground logout still clears credentials and local data.
      }
      await _store.clearForLogout();
      await service!.logout();
      state = AppState();
      syncError = null;
      notifyListeners();
    } finally {
      _loggingOut = false;
    }
  }

  Future<String> createList(String name, String emoji, String? photo) async {
    final id = _store.newId();
    await _store.append('list.created', id, {
      'name': name,
      'emoji': emoji,
      'photo': photo,
    });
    await _replay(localChange: true);
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
    await _replay(localChange: true);
  }

  Future<void> updateTask(
    String id,
    String title,
    String emoji,
    String? photo,
  ) async {
    final task = state.tasks[id];
    if (task == null || task.isCompleted || task.hasConflict) {
      throw StateError('This task is no longer open.');
    }
    await _store.append('task.updated', id, {
      'baseEventId': task.lastChangeEventId,
      'title': title,
      'emoji': emoji,
      'photo': photo,
    });
    await _replay(localChange: true);
  }

  Future<StoredEvent> completeTask(String id) async {
    final task = state.tasks[id];
    if (task == null || task.isCompleted || task.hasConflict) {
      throw StateError('This task is no longer open.');
    }
    // Capture the tap time before a permission prompt or GPS wait.
    final completedAt = DateTime.now().toUtc();
    final location = await _locationCapture();
    final event = await _store.append('task.completed', id, {
      'baseEventId': task.lastChangeEventId,
      'completedAtUtc': completedAt.toIso8601String(),
      'latitude': location.latitude,
      'longitude': location.longitude,
      'accuracyMeters': location.accuracyMeters,
      'locationStatus': location.status,
    });
    await _replay(localChange: true);
    return event;
  }

  Future<void> revertCompletion(String taskId, String completionEventId) async {
    final task = state.tasks[taskId];
    if (task?.completionEventId != completionEventId || task!.hasConflict) {
      return;
    }
    await _store.append('task.completion_reverted', taskId, {
      'baseEventId': task.lastChangeEventId,
      'completionEventId': completionEventId,
    });
    await _replay(localChange: true);
  }

  Future<void> reopenTask(String taskId) async {
    final task = state.tasks[taskId];
    if (task == null || !task.isCompleted || task.hasConflict) {
      throw StateError('This task cannot be reopened.');
    }
    await _store.append('task.reopened', taskId, {
      'baseEventId': task.lastChangeEventId,
      'completionEventId': task.completionEventId,
    });
    await _replay(localChange: true);
  }

  Future<void> resolveConflict(String taskId, {String? useEventId}) async {
    final task = state.tasks[taskId];
    if (task == null || !task.hasConflict) return;
    final snapshot = task.snapshot();
    if (useEventId != null) {
      final chosen = task.conflictingEvents.firstWhere(
        (event) => event.id == useEventId,
      );
      switch (chosen.type) {
        case 'task.updated':
          snapshot['title'] = chosen.payload['title'];
          snapshot['emoji'] = chosen.payload['emoji'];
          snapshot['photo'] = chosen.payload['photo'];
        case 'task.completed':
          snapshot['completionEventId'] = chosen.id;
          snapshot['completedAtUtc'] = chosen.payload['completedAtUtc'];
          snapshot['latitude'] = chosen.payload['latitude'];
          snapshot['longitude'] = chosen.payload['longitude'];
          snapshot['accuracyMeters'] = chosen.payload['accuracyMeters'];
          snapshot['locationStatus'] = chosen.payload['locationStatus'];
        case 'task.completion_reverted':
        case 'task.reopened':
          snapshot['completionEventId'] = null;
          snapshot['completedAtUtc'] = null;
          snapshot['latitude'] = null;
          snapshot['longitude'] = null;
          snapshot['accuracyMeters'] = null;
          snapshot['locationStatus'] = null;
        case 'task.conflict_resolved':
          snapshot.addAll(chosen.payload);
      }
    }
    await _store.append('task.conflict_resolved', taskId, {
      ...snapshot,
      'resolvedEventIds': [
        task.lastChangeEventId,
        ...task.conflictingEvents.map((event) => event.id),
      ],
    });
    await _replay(localChange: true);
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
