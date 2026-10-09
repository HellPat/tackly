import 'dart:async';
import 'dart:convert';

import 'package:cryptography/cryptography.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;
import 'package:uuid/uuid.dart';

import 'crypto_box.dart';
import 'event_store.dart';

class FamilyCredentials {
  const FamilyCredentials({
    required this.serverUrl,
    required this.familyId,
    required this.deviceToken,
    required this.familyKey,
    required this.recoverySecret,
    required this.owner,
  });

  final String serverUrl;
  final String familyId;
  final String deviceToken;
  final List<int> familyKey;
  final List<int> recoverySecret;
  final bool owner;

  static const _storage = FlutterSecureStorage();

  static Future<FamilyCredentials?> load() async {
    final value = await _storage.read(key: 'family_credentials_v1');
    if (value == null) return null;
    final json = jsonDecode(value) as Map<String, dynamic>;
    return FamilyCredentials(
      serverUrl: json['serverUrl'] as String,
      familyId: json['familyId'] as String,
      deviceToken: json['deviceToken'] as String,
      familyKey: decodeBytes(json['familyKey'] as String),
      recoverySecret: decodeBytes(json['recoverySecret'] as String),
      owner: json['owner'] as bool,
    );
  }

  Future<void> save() async {
    if (await load() != null) {
      throw StateError('This app is already connected to a family.');
    }
    await _storage.write(
      key: 'family_credentials_v1',
      value: jsonEncode({
        'serverUrl': serverUrl,
        'familyId': familyId,
        'deviceToken': deviceToken,
        'familyKey': encodeBytes(familyKey),
        'recoverySecret': encodeBytes(recoverySecret),
        'owner': owner,
      }),
    );
  }

  Future<void> update() async {
    await _storage.write(
      key: 'family_credentials_v1',
      value: jsonEncode({
        'serverUrl': serverUrl,
        'familyId': familyId,
        'deviceToken': deviceToken,
        'familyKey': encodeBytes(familyKey),
        'recoverySecret': encodeBytes(recoverySecret),
        'owner': owner,
      }),
    );
  }

  static Future<void> clear() => _storage.delete(key: 'family_credentials_v1');
}

class FamilyInvitation {
  const FamilyInvitation({
    required this.serverUrl,
    required this.familyId,
    required this.inviteId,
    required this.secret,
    required this.expiresAt,
  });

  final String serverUrl;
  final String familyId;
  final String inviteId;
  final List<int> secret;
  final DateTime expiresAt;

  Future<String> confirmationCode(String deviceId) async {
    final digest = (await Sha256().hash([...secret, ...utf8.encode(deviceId)]))
        .bytes;
    final number = ((digest[0] << 16) | (digest[1] << 8) | digest[2]) % 1000000;
    return number.toString().padLeft(6, '0');
  }

  String get qrValue => jsonEncode({
    'kind': 'tackly-family-invite-v1',
    'serverUrl': serverUrl,
    'familyId': familyId,
    'inviteId': inviteId,
    'secret': encodeBytes(secret),
    'expiresAt': expiresAt.toUtc().toIso8601String(),
  });

  factory FamilyInvitation.fromQr(String value) {
    final json = jsonDecode(value) as Map<String, dynamic>;
    if (json['kind'] != 'tackly-family-invite-v1') {
      throw const FormatException('This is not a Tackly invitation.');
    }
    final secret = decodeBytes(json['secret'] as String);
    if (secret.length != 32) throw const FormatException('Invalid invitation.');
    return FamilyInvitation(
      serverUrl: _safeServerUrl(json['serverUrl'] as String),
      familyId: json['familyId'] as String,
      inviteId: json['inviteId'] as String,
      secret: secret,
      expiresAt: DateTime.parse(json['expiresAt'] as String).toUtc(),
    );
  }
}

String _safeServerUrl(String input) {
  final url = Uri.parse(input.trim());
  if (!url.hasAuthority ||
      url.userInfo.isNotEmpty ||
      url.hasQuery ||
      url.hasFragment) {
    throw const FormatException('Enter a server URL.');
  }
  if (url.scheme != 'https' &&
      !(kDebugMode &&
          url.scheme == 'http' &&
          (url.host == 'localhost' ||
              url.host == '127.0.0.1' ||
              url.host == '10.0.2.2'))) {
    throw const FormatException('The server must use HTTPS.');
  }
  return url.toString().replaceFirst(RegExp(r'/$'), '');
}

class FamilySync {
  FamilySync(this.store, {http.Client? client})
    : _client = client ?? http.Client();

  final EventStore store;
  final http.Client _client;
  static const _ids = Uuid();
  FamilyCredentials? credentials;

  Future<void> load() async {
    credentials = await FamilyCredentials.load();
  }

  Future<void> logout() async {
    await FamilyCredentials.clear();
    credentials = null;
  }

  Future<Map<String, dynamic>> _request(
    String method,
    String url, {
    Object? body,
    String? token,
  }) async {
    final headers = <String, String>{'Content-Type': 'application/json'};
    if (token != null) headers['Authorization'] = 'Bearer $token';
    final request = http.Request(method, Uri.parse(url));
    request.headers.addAll(headers);
    if (body != null) request.body = jsonEncode(body);
    final streamed = await _client
        .send(request)
        .timeout(const Duration(seconds: 15));
    final response = await http.Response.fromStream(streamed);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw SyncHttpException(response.statusCode);
    }
    if (response.body.isEmpty) return {};
    return jsonDecode(response.body) as Map<String, dynamic>;
  }

  Future<void> createFamily(String serverInput, String familyName) async {
    if (credentials != null) throw StateError('Already connected to a family.');
    final serverUrl = serverInput.trim().isEmpty
        ? ''
        : _safeServerUrl(serverInput);
    final familyId = _ids.v4();
    final key = randomBytes(32);
    final recoverySecret = randomBytes(32);
    final created = FamilyCredentials(
      serverUrl: serverUrl,
      familyId: familyId,
      deviceToken: '',
      familyKey: key,
      recoverySecret: recoverySecret,
      owner: true,
    );
    await created.save();
    credentials = created;
    await store.append('family.created', familyId, {'name': familyName});
    try {
      await syncOnce();
    } catch (_) {
      // The family is already saved locally. The outbox will retry.
    }
  }

  Future<void> enableSync(String serverInput) async {
    final family = credentials;
    if (family == null || !family.owner || family.deviceToken.isNotEmpty) {
      throw StateError('This family is already connected to a server.');
    }
    final connected = FamilyCredentials(
      serverUrl: _safeServerUrl(serverInput),
      familyId: family.familyId,
      deviceToken: '',
      familyKey: family.familyKey,
      recoverySecret: family.recoverySecret,
      owner: true,
    );
    await connected.update();
    credentials = connected;
    await syncOnce();
  }

  Future<void> _registerLocalFamily() async {
    final family = credentials!;
    if (family.serverUrl.isEmpty || family.deviceToken.isNotEmpty) return;
    final response = await _request(
      'POST',
      '${family.serverUrl}/v1/families',
      body: {
        'family_id': family.familyId,
        'device_id': store.deviceId,
        'recovery_verifier': await _verifier(family.recoverySecret),
      },
    );
    final registered = FamilyCredentials(
      serverUrl: family.serverUrl,
      familyId: family.familyId,
      deviceToken: response['device_token'] as String,
      familyKey: family.familyKey,
      recoverySecret: family.recoverySecret,
      owner: family.owner,
    );
    await registered.update();
    credentials = registered;
  }

  Future<String> _verifier(List<int> secret) async =>
      encodeBytes((await Sha256().hash(secret)).bytes);

  Future<void> recoverFamily(String serverInput, String recovery) async {
    if (credentials != null) throw StateError('Already connected to a family.');
    final serverUrl = _safeServerUrl(serverInput);
    final parts = recovery.trim().split('.');
    if (parts.length != 4 || parts[0] != 'tackly1') {
      throw const FormatException('Invalid recovery key.');
    }
    final familyId = parts[1];
    final key = decodeBytes(parts[2]);
    final recoverySecret = decodeBytes(parts[3]);
    if (key.length != 32 ||
        recoverySecret.length != 32 ||
        !Uuid.isValidUUID(fromString: familyId)) {
      throw const FormatException('Invalid recovery key.');
    }
    final response = await _request(
      'POST',
      '$serverUrl/v1/families/$familyId/recover',
      body: {
        'device_id': store.deviceId,
        'recovery_verifier': await _verifier(recoverySecret),
      },
    );
    final recovered = FamilyCredentials(
      serverUrl: serverUrl,
      familyId: familyId,
      deviceToken: response['device_token'] as String,
      familyKey: key,
      recoverySecret: recoverySecret,
      owner: response['owner'] as bool,
    );
    await recovered.save();
    credentials = recovered;
    try {
      await syncOnce();
    } catch (_) {
      // Encrypted events remain on the server for the next retry.
    }
  }

  Future<FamilyInvitation> createInvitation() async {
    final family = credentials;
    if (family == null || !family.owner) {
      throw StateError('Family owner required.');
    }
    final inviteId = _ids.v4();
    final secret = randomBytes(32);
    final response = await _request(
      'POST',
      '${family.serverUrl}/v1/families/${family.familyId}/invites',
      token: family.deviceToken,
      body: {'invite_id': inviteId, 'verifier': await _verifier(secret)},
    );
    return FamilyInvitation(
      serverUrl: family.serverUrl,
      familyId: family.familyId,
      inviteId: inviteId,
      secret: secret,
      expiresAt: DateTime.parse(response['expires_at_utc'] as String).toUtc(),
    );
  }

  Future<String?> pendingJoinDevice(FamilyInvitation invitation) async {
    final family = credentials!;
    final result = await _request(
      'GET',
      '${family.serverUrl}/v1/families/${family.familyId}/invites/${invitation.inviteId}',
      token: family.deviceToken,
    );
    return result['status'] == 'pending'
        ? result['pending_device_id'] as String?
        : null;
  }

  Future<void> approveJoin(FamilyInvitation invitation, String deviceId) async {
    final family = credentials!;
    final package = await CryptoBox.seal(
      invitation.secret,
      family.familyKey,
      associatedData: '${invitation.inviteId}:$deviceId',
    );
    await _request(
      'POST',
      '${family.serverUrl}/v1/families/${family.familyId}/invites/${invitation.inviteId}/approve',
      token: family.deviceToken,
      body: {
        'device_id': deviceId,
        'package_nonce': package.nonce,
        'package_ciphertext': package.ciphertext,
        'package_mac': package.mac,
      },
    );
  }

  Future<void> cancelInvitation(FamilyInvitation invitation) async {
    final family = credentials!;
    await _request(
      'DELETE',
      '${family.serverUrl}/v1/families/${family.familyId}/invites/${invitation.inviteId}',
      token: family.deviceToken,
    );
  }

  Future<void> joinFamily(FamilyInvitation invitation) async {
    if (credentials != null) throw StateError('Already connected to a family.');
    if (DateTime.now().toUtc().isAfter(invitation.expiresAt)) {
      throw StateError('This invitation expired. Ask for a new one.');
    }
    final verifier = await _verifier(invitation.secret);
    final recoverySecret = randomBytes(32);
    await _request(
      'POST',
      '${invitation.serverUrl}/v1/invites/${invitation.inviteId}/request',
      body: {'verifier': verifier, 'device_id': store.deviceId},
    );
    while (DateTime.now().toUtc().isBefore(invitation.expiresAt)) {
      await Future<void>.delayed(const Duration(seconds: 2));
      try {
        final response = await _request(
          'POST',
          '${invitation.serverUrl}/v1/invites/${invitation.inviteId}/claim',
          body: {
            'verifier': verifier,
            'device_id': store.deviceId,
            'recovery_verifier': await _verifier(recoverySecret),
          },
        );
        final key = await CryptoBox.open(
          invitation.secret,
          SealedData(
            nonce: response['package_nonce'] as String,
            ciphertext: response['package_ciphertext'] as String,
            mac: response['package_mac'] as String,
          ),
          associatedData: '${invitation.inviteId}:${store.deviceId}',
        );
        if (key.length != 32 || response['family_id'] != invitation.familyId) {
          throw const FormatException('Invalid family key package.');
        }
        final joined = FamilyCredentials(
          serverUrl: invitation.serverUrl,
          familyId: invitation.familyId,
          deviceToken: response['device_token'] as String,
          familyKey: key,
          recoverySecret: recoverySecret,
          owner: false,
        );
        await joined.save();
        credentials = joined;
        try {
          await syncOnce();
        } catch (_) {
          // A joined device can read cached events after reconnecting.
        }
        return;
      } on SyncHttpException catch (error) {
        if (error.statusCode != 410) rethrow;
      }
    }
    throw StateError('This invitation expired. Ask for a new one.');
  }

  Future<bool> syncOnce() async {
    if (credentials == null || credentials!.serverUrl.isEmpty) return false;
    await _registerLocalFamily();
    final family = credentials!;
    final endpoint =
        '${family.serverUrl}/v1/families/${family.familyId}/events';
    for (final event in await store.pendingEvents()) {
      final box = await store.outboxEnvelope(
        event,
        family.familyId,
        family.familyKey,
      );
      await _request(
        'POST',
        endpoint,
        token: family.deviceToken,
        body: {
          'events': [
            {
              'event_id': event.id,
              'aggregate_id': event.aggregateId,
              'origin_device_id': event.originDeviceId,
              'key_version': 1,
              ...box.toJson(),
            },
          ],
        },
      );
      await store.markPushed(event.id);
    }
    var changed = false;
    while (true) {
      final cursor = await store.syncCursor();
      final response = await _request(
        'GET',
        '$endpoint?after=$cursor&limit=20',
        token: family.deviceToken,
      );
      final raw = response['events'] as List<dynamic>;
      if (raw.isEmpty) break;
      final events = <(StoredEvent, int)>[];
      var lastSequence = cursor;
      for (final item in raw) {
        final json = item as Map<String, dynamic>;
        final id = json['event_id'] as String;
        final aggregateId = json['aggregate_id'] as String;
        final origin = json['origin_device_id'] as String;
        final clear = await CryptoBox.open(
          family.familyKey,
          SealedData(
            nonce: json['nonce'] as String,
            ciphertext: json['ciphertext'] as String,
            mac: json['mac'] as String,
          ),
          associatedData: '${family.familyId}:$id:$aggregateId:$origin',
        );
        final body = jsonDecode(utf8.decode(clear)) as Map<String, dynamic>;
        final serverSequence = json['sequence'] as int;
        events.add((
          StoredEvent(
            id: id,
            type: body['type'] as String,
            aggregateId: aggregateId,
            originDeviceId: origin,
            occurredAtUtc: DateTime.parse(body['occurredAtUtc'] as String),
            payload: Map<String, Object?>.from(body['payload'] as Map),
          ),
          serverSequence,
        ));
        lastSequence = serverSequence;
      }
      await store.importEvents(events, lastSequence);
      changed = true;
      if (raw.length < 20) break;
    }
    return changed;
  }

  void close() => _client.close();
}

class SyncHttpException implements Exception {
  const SyncHttpException(this.statusCode);
  final int statusCode;

  @override
  String toString() => 'Sync server returned $statusCode';
}
