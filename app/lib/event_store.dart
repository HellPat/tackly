import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:sqflite/sqflite.dart';
import 'package:uuid/uuid.dart';

import 'crypto_box.dart';

/// The database contains events only. Screens are rebuilt by replaying them.
class EventStore {
  EventStore._(this._database, this._localKey, this.deviceId);

  final Database _database;
  List<int> _localKey;
  String deviceId;
  static const _ids = Uuid();
  static const _secrets = FlutterSecureStorage();

  static Future<EventStore> open({
    String databaseName = 'tackly_events.db',
    String? deviceIdOverride,
  }) async {
    var encodedKey = await _secrets.read(key: 'local_event_key');
    if (encodedKey == null) {
      encodedKey = encodeBytes(randomBytes(32));
      await _secrets.write(key: 'local_event_key', value: encodedKey);
    }
    final localKey = decodeBytes(encodedKey);
    var deviceId = await _secrets.read(key: 'device_id');
    if (deviceId == null) {
      deviceId = _ids.v4();
      await _secrets.write(key: 'device_id', value: deviceId);
    }
    final eventDeviceId = deviceIdOverride ?? deviceId;
    final directory = await getDatabasesPath();
    var migratedPlaintext = false;
    final database = await openDatabase(
      '$directory/$databaseName',
      version: 2,
      onCreate: (db, version) => _createSchema(db),
      onUpgrade: (db, oldVersion, newVersion) async {
        if (oldVersion != 1) throw StateError('Unsupported database version');
        await db.execute('ALTER TABLE events RENAME TO old_events');
        await _createSchema(db);
        final rows = await db.query('old_events', orderBy: 'sequence ASC');
        for (final row in rows) {
          final event = StoredEvent.fromLegacyRow(row, eventDeviceId);
          final sealed = await _sealLocal(localKey, event);
          await db.insert('events', {'sequence': row['sequence'], ...sealed});
        }
        await db.execute('DROP TABLE old_events');
        migratedPlaintext = true;
      },
    );
    // SQLite may retain dropped plaintext pages from the v1 migration until
    // vacuumed. This also truncates the rollback journal after migration.
    if (migratedPlaintext) await database.execute('VACUUM');
    return EventStore._(database, localKey, eventDeviceId);
  }

  static Future<void> _createSchema(DatabaseExecutor db) async {
    await db.execute('''
          CREATE TABLE events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            schema_version INTEGER NOT NULL,
            aggregate_id TEXT NOT NULL,
            occurred_at_utc TEXT NOT NULL,
            origin_device_id TEXT NOT NULL,
            local_nonce TEXT NOT NULL,
            local_ciphertext TEXT NOT NULL,
            local_mac TEXT NOT NULL,
            server_sequence INTEGER,
            pushed INTEGER NOT NULL DEFAULT 0
          )
        ''');
    await db.execute('''
      CREATE TABLE sync_metadata (
        name TEXT PRIMARY KEY,
        value TEXT NOT NULL
      )
    ''');
    await db.execute('''
      CREATE TABLE sync_outbox (
        event_id TEXT PRIMARY KEY REFERENCES events(event_id),
        nonce TEXT NOT NULL,
        ciphertext TEXT NOT NULL,
        mac TEXT NOT NULL
      )
    ''');
  }

  String newId() => _ids.v4();

  static Future<Map<String, Object?>> _sealLocal(
    List<int> key,
    StoredEvent event,
  ) async {
    final sealed = await CryptoBox.seal(
      key,
      utf8.encode(
        jsonEncode({
          'type': event.type,
          'payload': event.payload,
          'occurredAtUtc': event.occurredAtUtc.toIso8601String(),
        }),
      ),
      associatedData: event.id,
    );
    return {
      'event_id': event.id,
      'schema_version': 1,
      'aggregate_id': event.aggregateId,
      'occurred_at_utc': event.occurredAtUtc.toIso8601String(),
      'origin_device_id': event.originDeviceId,
      'local_nonce': sealed.nonce,
      'local_ciphertext': sealed.ciphertext,
      'local_mac': sealed.mac,
    };
  }

  Future<StoredEvent> _openLocal(Map<String, Object?> row) async {
    if (row['schema_version'] != 1) {
      throw FormatException(
        'Unsupported event schema: ${row['schema_version']}',
      );
    }
    final id = row['event_id'] as String;
    final clear = await CryptoBox.open(
      _localKey,
      SealedData(
        nonce: row['local_nonce'] as String,
        ciphertext: row['local_ciphertext'] as String,
        mac: row['local_mac'] as String,
      ),
      associatedData: id,
    );
    final body = jsonDecode(utf8.decode(clear)) as Map<String, dynamic>;
    return StoredEvent(
      id: id,
      type: body['type'] as String,
      aggregateId: row['aggregate_id'] as String,
      originDeviceId: row['origin_device_id'] as String,
      occurredAtUtc: DateTime.parse(row['occurred_at_utc'] as String),
      payload: Map<String, Object?>.from(body['payload'] as Map),
    );
  }

  Future<StoredEvent> append(
    String type,
    String aggregateId,
    Map<String, Object?> payload,
  ) async {
    final event = StoredEvent(
      id: newId(),
      type: type,
      aggregateId: aggregateId,
      originDeviceId: deviceId,
      occurredAtUtc: DateTime.now().toUtc(),
      payload: payload,
    );
    await _database.insert('events', await _sealLocal(_localKey, event));
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
    final rows = await _database.query(
      'events',
      orderBy: 'server_sequence IS NULL, server_sequence ASC, sequence ASC',
    );
    return Future.wait(rows.map(_openLocal));
  }

  Future<List<StoredEvent>> pendingEvents() async {
    final rows = await _database.query(
      'events',
      where: 'pushed = 0 AND origin_device_id = ?',
      whereArgs: [deviceId],
      orderBy: 'sequence ASC',
    );
    return Future.wait(rows.map(_openLocal));
  }

  /// Forget this phone's family data and identity before another family joins.
  Future<void> clearForLogout() async {
    final nextKey = randomBytes(32);
    final nextDeviceId = _ids.v4();
    await _database.rawQuery('PRAGMA secure_delete = ON');
    await _database.transaction((tx) async {
      await tx.delete('sync_outbox');
      await tx.delete('events');
      await tx.delete('sync_metadata');
    });
    await _database.execute('VACUUM');
    await _secrets.write(key: 'local_event_key', value: encodeBytes(nextKey));
    await _secrets.write(key: 'device_id', value: nextDeviceId);
    _localKey = nextKey;
    deviceId = nextDeviceId;
  }

  Future<void> markPushed(String eventId) async {
    await _database.update(
      'events',
      {'pushed': 1},
      where: 'event_id = ?',
      whereArgs: [eventId],
    );
  }

  Future<SealedData> outboxEnvelope(
    StoredEvent event,
    String familyId,
    List<int> familyKey,
  ) async {
    final saved = await _database.query(
      'sync_outbox',
      where: 'event_id = ?',
      whereArgs: [event.id],
    );
    if (saved.isNotEmpty) {
      final row = saved.single;
      return SealedData(
        nonce: row['nonce'] as String,
        ciphertext: row['ciphertext'] as String,
        mac: row['mac'] as String,
      );
    }
    final sealed = await CryptoBox.seal(
      familyKey,
      utf8.encode(
        jsonEncode({
          'type': event.type,
          'payload': event.payload,
          'occurredAtUtc': event.occurredAtUtc.toIso8601String(),
        }),
      ),
      associatedData:
          '$familyId:${event.id}:${event.aggregateId}:${event.originDeviceId}',
    );
    await _database.insert('sync_outbox', {
      'event_id': event.id,
      ...sealed.toJson(),
    }, conflictAlgorithm: ConflictAlgorithm.ignore);
    // Another sync worker can have created the envelope first. Always send
    // the durable copy so a retry uses precisely the same ciphertext.
    final durable = (await _database.query(
      'sync_outbox',
      where: 'event_id = ?',
      whereArgs: [event.id],
    )).single;
    return SealedData(
      nonce: durable['nonce'] as String,
      ciphertext: durable['ciphertext'] as String,
      mac: durable['mac'] as String,
    );
  }

  Future<int> syncCursor() async {
    final rows = await _database.query(
      'sync_metadata',
      where: 'name = ?',
      whereArgs: ['cursor'],
    );
    return rows.isEmpty ? 0 : int.parse(rows.single['value'] as String);
  }

  Future<void> importEvents(List<(StoredEvent, int)> events, int cursor) async {
    final rows = <Map<String, Object?>>[];
    for (final (event, serverSequence) in events) {
      rows.add({
        ...await _sealLocal(_localKey, event),
        'server_sequence': serverSequence,
      });
    }
    await _database.transaction((tx) async {
      for (final row in rows) {
        await tx.insert('events', {
          ...row,
          'pushed': 1,
        }, conflictAlgorithm: ConflictAlgorithm.ignore);
        await tx.update(
          'events',
          {'server_sequence': row['server_sequence'], 'pushed': 1},
          where: 'event_id = ?',
          whereArgs: [row['event_id']],
        );
      }
      await tx.insert('sync_metadata', {
        'name': 'cursor',
        'value': '$cursor',
      }, conflictAlgorithm: ConflictAlgorithm.replace);
    });
  }

  Future<void> close() => _database.close();
}

class StoredEvent {
  const StoredEvent({
    required this.id,
    required this.type,
    required this.aggregateId,
    required this.originDeviceId,
    required this.occurredAtUtc,
    required this.payload,
  });

  final String id;
  final String type;
  final String aggregateId;
  final String originDeviceId;
  final DateTime occurredAtUtc;
  final Map<String, Object?> payload;

  factory StoredEvent.fromLegacyRow(
    Map<String, Object?> row,
    String deviceId,
  ) => StoredEvent(
    id: row['event_id'] as String,
    type: row['event_type'] as String,
    aggregateId: row['aggregate_id'] as String,
    originDeviceId: deviceId,
    occurredAtUtc: DateTime.parse(row['occurred_at_utc'] as String),
    payload: Map<String, Object?>.from(
      jsonDecode(row['payload_json'] as String) as Map,
    ),
  );
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
  String? lastChangeEventId;
  final List<StoredEvent> conflictingEvents = [];

  bool get isCompleted => completionEventId != null;
  bool get hasConflict => conflictingEvents.isNotEmpty;

  Map<String, Object?> snapshot() => {
    'title': title,
    'emoji': emoji,
    'photo': photo,
    'completionEventId': completionEventId,
    'completedAtUtc': completedAtUtc?.toIso8601String(),
    'latitude': latitude,
    'longitude': longitude,
    'accuracyMeters': accuracyMeters,
    'locationStatus': locationStatus,
  };

  void restore(Map<String, Object?> payload) {
    title = payload['title'] as String;
    emoji = payload['emoji'] as String;
    photo = payload['photo'] as String?;
    completionEventId = payload['completionEventId'] as String?;
    completedAtUtc = payload['completedAtUtc'] == null
        ? null
        : DateTime.parse(payload['completedAtUtc'] as String);
    latitude = (payload['latitude'] as num?)?.toDouble();
    longitude = (payload['longitude'] as num?)?.toDouble();
    accuracyMeters = (payload['accuracyMeters'] as num?)?.toDouble();
    locationStatus = payload['locationStatus'] as String?;
  }

  bool accepts(StoredEvent event) {
    if (hasConflict) {
      conflictingEvents.add(event);
      return false;
    }
    final base = event.payload['baseEventId'] as String?;
    if (base != null && base != lastChangeEventId) {
      conflictingEvents.add(event);
      return false;
    }
    lastChangeEventId = event.id;
    return true;
  }
}

class AppState {
  String? familyName;
  final Map<String, ListItem> lists = {};
  final Map<String, TaskItem> tasks = {};

  Iterable<TaskItem> openTasks({String? listId}) => tasks.values.where(
    (task) =>
        (!task.isCompleted || task.hasConflict) &&
        (listId == null || task.listId == listId),
  );

  void apply(StoredEvent event) {
    final payload = event.payload;
    switch (event.type) {
      case 'family.created':
        familyName = payload['name'] as String;
        return;
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
        )..lastChangeEventId = event.id;
        return;
      case 'task.updated':
        final task = tasks[event.aggregateId];
        if (task == null) throw FormatException('Update for missing task');
        if (!task.accepts(event)) return;
        task.title = payload['title'] as String;
        task.emoji = payload['emoji'] as String;
        task.photo = payload['photo'] as String?;
        return;
      case 'task.completed':
        final task = tasks[event.aggregateId];
        if (task == null) throw FormatException('Completion for missing task');
        if (!task.accepts(event)) return;
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
        if (task == null) throw FormatException('Revert for missing task');
        if (!task.accepts(event)) return;
        if (task.completionEventId != payload['completionEventId'] &&
            !task.hasConflict) {
          task.conflictingEvents.add(event);
          return;
        }
        task.completionEventId = null;
        task.completedAtUtc = null;
        task.latitude = null;
        task.longitude = null;
        task.accuracyMeters = null;
        task.locationStatus = null;
        return;
      case 'task.conflict_resolved':
        final task = tasks[event.aggregateId];
        if (task == null) throw FormatException('Resolution for missing task');
        final covered = (payload['resolvedEventIds'] as List<dynamic>)
            .cast<String>()
            .toSet();
        if (!covered.contains(task.lastChangeEventId) ||
            task.conflictingEvents.any(
              (other) => !covered.contains(other.id),
            )) {
          task.conflictingEvents.add(event);
          return;
        }
        task.restore(payload);
        task.conflictingEvents.clear();
        task.lastChangeEventId = event.id;
        return;
      default:
        throw FormatException('Unknown event type: ${event.type}');
    }
  }
}
