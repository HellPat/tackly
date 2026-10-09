import 'dart:async';

import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:qr_flutter/qr_flutter.dart';

import 'app_controller.dart';
import 'background_sync.dart';
import 'crypto_box.dart';
import 'family_sync.dart';

class FamilyPage extends StatefulWidget {
  const FamilyPage({
    super.key,
    required this.controller,
    this.onConnected,
    this.onLoggedOut,
    this.invitationOnly = false,
  });
  final AppController controller;
  final VoidCallback? onConnected;
  final VoidCallback? onLoggedOut;
  final bool invitationOnly;

  @override
  State<FamilyPage> createState() => _FamilyPageState();
}

class _FamilyPageState extends State<FamilyPage> {
  FamilyInvitation? _invitation;
  Timer? _poll;
  bool _busy = false;
  bool _checkingInvite = false;
  String? _message;
  String? _pendingDevice;

  FamilySync get _service => widget.controller.familySync!;

  @override
  void dispose() {
    _poll?.cancel();
    final invite = _invitation;
    if (invite != null) {
      unawaited(_service.cancelInvitation(invite).catchError((Object _) {}));
    }
    super.dispose();
  }

  Future<void> _createFamily() async {
    final url = TextEditingController();
    final name = TextEditingController(text: 'Our Home');
    final submitted = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Create family'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: name,
              maxLength: 40,
              decoration: const InputDecoration(labelText: 'Family name'),
            ),
            TextField(
              controller: url,
              keyboardType: TextInputType.url,
              decoration: const InputDecoration(
                labelText: 'Sync server URL (optional)',
                hintText: 'https://sync.example.com',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Create'),
          ),
        ],
      ),
    );
    if (submitted != true) {
      url.dispose();
      name.dispose();
      return;
    }
    setState(() => _busy = true);
    try {
      await _service.createFamily(url.text.trim(), name.text.trim());
      try {
        await scheduleBackgroundSync();
      } catch (_) {}
      await widget.controller.refreshFromDisk();
      await widget.controller.syncNow();
      if (mounted) await _showRecoveryKey();
      if (mounted) {
        widget.onConnected?.call();
      }
    } catch (error) {
      if (mounted) setState(() => _message = 'Could not create family: $error');
    } finally {
      url.dispose();
      name.dispose();
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _joinFamily() async {
    final qr = await Navigator.push<String>(
      context,
      MaterialPageRoute(builder: (_) => const _ScanInvitePage()),
    );
    if (qr == null || !mounted) return;
    late final FamilyInvitation invitation;
    late final String confirmationCode;
    try {
      invitation = FamilyInvitation.fromQr(qr);
      confirmationCode = await invitation.confirmationCode(
        _service.store.deviceId,
      );
    } catch (error) {
      if (mounted) setState(() => _message = 'Invalid invitation: $error');
      return;
    }
    if (!mounted) return;
    setState(() {
      _busy = true;
      _message = 'Waiting for confirmation · code $confirmationCode';
    });
    try {
      await _service.joinFamily(invitation);
      try {
        await scheduleBackgroundSync();
      } catch (_) {}
      await widget.controller.refreshFromDisk();
      await widget.controller.syncNow();
      if (mounted) {
        widget.onConnected?.call();
      }
    } catch (error) {
      if (mounted) setState(() => _message = 'Could not join: $error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _recoverFamily() async {
    final url = TextEditingController();
    final recovery = TextEditingController();
    final submitted = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Restore family'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: url,
              keyboardType: TextInputType.url,
              decoration: const InputDecoration(labelText: 'Sync server URL'),
            ),
            TextField(
              controller: recovery,
              decoration: const InputDecoration(labelText: 'Recovery key'),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Restore'),
          ),
        ],
      ),
    );
    if (submitted != true) {
      url.dispose();
      recovery.dispose();
      return;
    }
    setState(() => _busy = true);
    try {
      await _service.recoverFamily(url.text.trim(), recovery.text.trim());
      try {
        await scheduleBackgroundSync();
      } catch (_) {}
      await widget.controller.refreshFromDisk();
      await widget.controller.syncNow();
      if (mounted) {
        widget.onConnected?.call();
      }
    } catch (error) {
      if (mounted) setState(() => _message = 'Could not restore: $error');
    } finally {
      url.dispose();
      recovery.dispose();
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _showRecoveryKey() async {
    final family = _service.credentials;
    if (family == null || !mounted) return;
    final recovery =
        'tackly1.${family.familyId}.${encodeBytes(family.familyKey)}.${encodeBytes(family.recoverySecret)}';
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (context) => AlertDialog(
        title: const Text('Save your recovery key'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Text(
              'Save this in your password manager. The server cannot recover it.',
            ),
            const SizedBox(height: 16),
            SelectableText(recovery),
          ],
        ),
        actions: [
          FilledButton(
            onPressed: () => Navigator.pop(context),
            child: const Text("I've saved it"),
          ),
        ],
      ),
    );
  }

  Future<void> _invite() async {
    setState(() {
      _busy = true;
      _message = null;
      _pendingDevice = null;
    });
    try {
      final invite = await _service.createInvitation();
      if (!mounted) return;
      setState(() => _invitation = invite);
      _poll?.cancel();
      _poll = Timer.periodic(
        const Duration(seconds: 2),
        (_) => unawaited(_checkInvite()),
      );
    } catch (error) {
      if (mounted) {
        setState(() => _message = 'Could not create invitation: $error');
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _connectServer() async {
    final url = TextEditingController(text: _service.credentials?.serverUrl);
    final submitted = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Connect sync server'),
        content: TextField(
          controller: url,
          keyboardType: TextInputType.url,
          decoration: const InputDecoration(labelText: 'Sync server URL'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Connect'),
          ),
        ],
      ),
    );
    if (submitted != true) {
      url.dispose();
      return;
    }
    setState(() => _busy = true);
    try {
      await _service.enableSync(url.text.trim());
      await scheduleBackgroundSync();
      if (mounted) setState(() => _message = 'Family is ready to share.');
    } catch (error) {
      if (mounted) {
        setState(
          () => _message = _service.credentials?.serverUrl.isNotEmpty == true
              ? 'Saved the server address. Connection will retry when available: $error'
              : 'Could not save the server address: $error',
        );
      }
    } finally {
      url.dispose();
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _checkInvite() async {
    final invite = _invitation;
    if (invite == null || _pendingDevice != null || _checkingInvite) return;
    if (DateTime.now().toUtc().isAfter(invite.expiresAt)) {
      _poll?.cancel();
      if (mounted) {
        setState(() => _message = 'Invitation expired. Create a new one.');
      }
      return;
    }
    _checkingInvite = true;
    try {
      final device = await _service.pendingJoinDevice(invite);
      if (device == null || !mounted) return;
      _poll?.cancel();
      final confirmationCode = await invite.confirmationCode(device);
      if (!mounted) return;
      final approved = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: const Text('Join request'),
          content: Text(
            'Compare code $confirmationCode with the code on her phone. Allow only if they match.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Allow'),
            ),
          ],
        ),
      );
      if (approved == true) {
        await _service.approveJoin(invite, device);
        if (mounted) {
          setState(() {
            _pendingDevice = device;
            _message = 'Approved. Her phone can now join.';
            _invitation = null;
          });
        }
      } else {
        await _service.cancelInvitation(invite);
        if (mounted) {
          setState(() {
            _invitation = null;
            _message = 'Invitation cancelled.';
          });
        }
      }
    } catch (error) {
      if (mounted) setState(() => _message = 'Invitation check failed: $error');
    } finally {
      _checkingInvite = false;
    }
  }

  Future<void> _logout() async {
    final pending = await widget.controller.pendingChangeCount();
    if (!mounted) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Log out of family?'),
        content: Text(
          'This removes the family data and keys from this phone. '
          'You will need your recovery key or a new invitation to return.'
          '${pending == 0 ? '' : ' $pending changes have not synced and will be lost.'}',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Log out'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    setState(() => _busy = true);
    try {
      await widget.controller.logout();
      if (!mounted) return;
      Navigator.of(context).pop();
      widget.onLoggedOut?.call();
    } catch (error) {
      if (mounted) setState(() => _message = 'Could not log out: $error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: widget.controller,
    builder: (context, _) {
      final family = _service.credentials;
      return Scaffold(
        appBar: AppBar(
          title: Text(widget.invitationOnly ? 'Invite' : 'Family'),
        ),
        body: ListView(
          padding: const EdgeInsets.all(20),
          children: [
            if (family == null) ...[
              FilledButton(
                onPressed: _busy ? null : _createFamily,
                child: const Text('Create Family'),
              ),
              const SizedBox(height: 12),
              OutlinedButton(
                onPressed: _busy ? null : _joinFamily,
                child: const Text('Scan Invitation Code'),
              ),
              const SizedBox(height: 12),
              TextButton(
                onPressed: _busy ? null : _recoverFamily,
                child: const Text('Restore with recovery key'),
              ),
            ] else if (widget.invitationOnly) ...[
              if (family.serverUrl.isEmpty)
                FilledButton(
                  onPressed: _busy ? null : _connectServer,
                  child: const Text('Connect sync server'),
                ),
              if (family.serverUrl.isNotEmpty && family.deviceToken.isEmpty)
                Column(
                  children: [
                    const Text('Waiting for the sync server to connect.'),
                    OutlinedButton(
                      onPressed: _busy ? null : _connectServer,
                      child: const Text('Change sync server'),
                    ),
                    TextButton(
                      onPressed: _busy ? null : widget.controller.syncNow,
                      child: const Text('Retry connection'),
                    ),
                  ],
                ),
              if (family.owner &&
                  family.deviceToken.isNotEmpty &&
                  _invitation == null)
                FilledButton(
                  onPressed: _busy ? null : _invite,
                  child: const Text('Create invitation'),
                ),
              if (_invitation case final invite?) ...[
                const Text(
                  'Have her scan this code. It expires after five minutes.',
                ),
                const SizedBox(height: 16),
                Center(
                  child: QrImageView(
                    data: invite.qrValue,
                    size: 240,
                    backgroundColor: Colors.white,
                  ),
                ),
                const SizedBox(height: 16),
                OutlinedButton(
                  onPressed: () async {
                    _poll?.cancel();
                    await _service.cancelInvitation(invite);
                    if (mounted) setState(() => _invitation = null);
                  },
                  child: const Text('Cancel invitation'),
                ),
              ],
            ] else ...[
              OutlinedButton(
                onPressed: _busy ? null : _logout,
                child: const Text('Logout'),
              ),
            ],
            if (_busy)
              const Padding(
                padding: EdgeInsets.only(top: 20),
                child: Center(child: CircularProgressIndicator()),
              ),
            if (_message != null)
              Padding(
                padding: const EdgeInsets.only(top: 20),
                child: Text(_message!),
              ),
          ],
        ),
      );
    },
  );
}

class _ScanInvitePage extends StatefulWidget {
  const _ScanInvitePage();

  @override
  State<_ScanInvitePage> createState() => _ScanInvitePageState();
}

class _ScanInvitePageState extends State<_ScanInvitePage> {
  bool _found = false;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Scan invitation')),
    body: MobileScanner(
      onDetect: (capture) {
        if (_found || capture.barcodes.isEmpty) return;
        final value = capture.barcodes.first.rawValue;
        if (value == null) return;
        _found = true;
        Navigator.pop(context, value);
      },
    ),
  );
}
