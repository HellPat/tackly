import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:cryptography/cryptography.dart';

String encodeBytes(List<int> bytes) =>
    base64UrlEncode(bytes).replaceAll('=', '');

Uint8List decodeBytes(String value) =>
    Uint8List.fromList(base64Url.decode(base64Url.normalize(value)));

Uint8List randomBytes(int count) {
  final random = Random.secure();
  return Uint8List.fromList(
    List<int>.generate(count, (_) => random.nextInt(256)),
  );
}

class SealedData {
  const SealedData({
    required this.nonce,
    required this.ciphertext,
    required this.mac,
  });

  final String nonce;
  final String ciphertext;
  final String mac;

  Map<String, String> toJson() => {
    'nonce': nonce,
    'ciphertext': ciphertext,
    'mac': mac,
  };
}

class CryptoBox {
  static final _aes = AesGcm.with256bits();

  static Future<SealedData> seal(
    List<int> key,
    List<int> plaintext, {
    required String associatedData,
  }) async {
    final nonce = randomBytes(12);
    final box = await _aes.encrypt(
      plaintext,
      secretKey: SecretKey(key),
      nonce: nonce,
      aad: utf8.encode(associatedData),
    );
    return SealedData(
      nonce: encodeBytes(nonce),
      ciphertext: encodeBytes(box.cipherText),
      mac: encodeBytes(box.mac.bytes),
    );
  }

  static Future<Uint8List> open(
    List<int> key,
    SealedData data, {
    required String associatedData,
  }) async {
    final clear = await _aes.decrypt(
      SecretBox(
        decodeBytes(data.ciphertext),
        nonce: decodeBytes(data.nonce),
        mac: Mac(decodeBytes(data.mac)),
      ),
      secretKey: SecretKey(key),
      aad: utf8.encode(associatedData),
    );
    return Uint8List.fromList(clear);
  }
}
