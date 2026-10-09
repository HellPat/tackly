import 'dart:convert';

import 'package:sqflite/sqflite.dart';
import 'package:uuid/uuid.dart';

/// The database contains events only. Screens are rebuilt by replaying them.
class EventStore {
  EventStore._(this._database);

  final Database _database;
  static const _ids = Uuid();

  static Future<EventStore> open({
    String databaseName = 'tackly_events.db',
  }) async {
    final directory = await getDatabasesPath();
    final database = await openDatabase(
      '$directory/$databaseName',
      version: 1,
      onCreate: (db, version) async {
        await db.execute('''
          CREATE TABLE events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            schema_version INTEGER NOT NULL,
            aggregate_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            occurred_at_utc TEXT NOT NULL,
            payload_json TEXT NOT NULL
          )
        ''');
      },
    );
    return EventStore._(database);
  }

  String newId() => _ids.v4();

  Future<StoredEvent> append(
    String type,
    String aggregateId,
    Map<String, Object?> payload,
  ) async {
    final event = StoredEvent(
      id: newId(),
      type: type,
      aggregateId: aggregateId,
      occurredAtUtc: DateTime.now().toUtc(),
      payload: payload,
    );
    await _database.insert('events', event.toRow());
    return event;
  }

  Future<AppState> load() async {
    final state = AppState();
    for (final event in await readEvents()) {
      state.apply(event);
    }
    return state;
  }

  /// Ordered source of truth for replay and eventual event synchronization.
  Future<List<StoredEvent>> readEvents() async {
    final rows = await _database.query('events', orderBy: 'sequence ASC');
    return rows.map(StoredEvent.fromRow).toList();
  }

  Future<void> close() => _database.close();
}

class StoredEvent {
  const StoredEvent({
    required this.id,
    required this.type,
    required this.aggregateId,
    required this.occurredAtUtc,
    required this.payload,
  });

  final String id;
  final String type;
  final String aggregateId;
  final DateTime occurredAtUtc;
  final Map<String, Object?> payload;

  Map<String, Object?> toRow() => {
    'event_id': id,
    'schema_version': 1,
    'aggregate_id': aggregateId,
    'event_type': type,
    'occurred_at_utc': occurredAtUtc.toIso8601String(),
    'payload_json': jsonEncode(payload),
  };

  factory StoredEvent.fromRow(Map<String, Object?> row) {
    if (row['schema_version'] != 1) {
      throw FormatException(
        'Unsupported event schema: ${row['schema_version']}',
      );
    }
    return StoredEvent(
      id: row['event_id'] as String,
      type: row['event_type'] as String,
      aggregateId: row['aggregate_id'] as String,
      occurredAtUtc: DateTime.parse(row['occurred_at_utc'] as String),
      payload: Map<String, Object?>.from(
        jsonDecode(row['payload_json'] as String) as Map,
      ),
    );
  }
}

class ListItem {
  ListItem({
    required this.id,
    required this.name,
    required this.emoji,
    this.photo,
  });

  final String id;
  final String name;
  final String emoji;
  final String? photo;
}

class TaskItem {
  TaskItem({
    required this.id,
    required this.listId,
    required this.title,
    required this.emoji,
    this.photo,
  });

  final String id;
  final String listId;
  String title;
  String emoji;
  String? photo;
  String? completionEventId;
  DateTime? completedAtUtc;
  double? latitude;
  double? longitude;
  double? accuracyMeters;
  String? locationStatus;

  bool get isCompleted => completionEventId != null;
}

class AppState {
  final Map<String, ListItem> lists = {};
  final Map<String, TaskItem> tasks = {};

  Iterable<TaskItem> openTasks({String? listId}) => tasks.values.where(
    (task) => !task.isCompleted && (listId == null || task.listId == listId),
  );

  void apply(StoredEvent event) {
    final payload = event.payload;
    switch (event.type) {
      case 'list.created':
        lists[event.aggregateId] = ListItem(
          id: event.aggregateId,
          name: payload['name'] as String,
          emoji: payload['emoji'] as String,
          photo: payload['photo'] as String?,
        );
        return;
      case 'task.created':
        tasks[event.aggregateId] = TaskItem(
          id: event.aggregateId,
          listId: payload['listId'] as String,
          title: payload['title'] as String,
          emoji: payload['emoji'] as String,
          photo: payload['photo'] as String?,
        );
        return;
      case 'task.updated':
        final task = tasks[event.aggregateId];
        if (task == null) throw FormatException('Update for missing task');
        task.title = payload['title'] as String;
        task.emoji = payload['emoji'] as String;
        task.photo = payload['photo'] as String?;
        return;
      case 'task.completed':
        final task = tasks[event.aggregateId];
        if (task == null) throw FormatException('Completion for missing task');
        task.completionEventId = event.id;
        task.completedAtUtc = DateTime.parse(
          payload['completedAtUtc'] as String,
        );
        task.latitude = (payload['latitude'] as num?)?.toDouble();
        task.longitude = (payload['longitude'] as num?)?.toDouble();
        task.accuracyMeters = (payload['accuracyMeters'] as num?)?.toDouble();
        task.locationStatus = payload['locationStatus'] as String;
        return;
      case 'task.completion_reverted':
        final task = tasks[event.aggregateId];
        if (task == null ||
            task.completionEventId != payload['completionEventId']) {
          throw FormatException('Revert does not match active completion');
        }
        task.completionEventId = null;
        task.completedAtUtc = null;
        task.latitude = null;
        task.longitude = null;
        task.accuracyMeters = null;
        task.locationStatus = null;
        return;
      default:
        throw FormatException('Unknown event type: ${event.type}');
    }
  }
}
