import base64, concurrent.futures, hashlib, json, os, secrets, urllib.error, urllib.request, uuid

BASE = os.environ.get('TACKLY_TEST_BASE_URL', 'http://127.0.0.1:3000')

def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode()

def call(method, path, body=None, token=None):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    request = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())

family, owner, wife = str(uuid.uuid4()), str(uuid.uuid4()), str(uuid.uuid4())
verifier = b64(hashlib.sha256(secrets.token_bytes(32)).digest())
status, created = call('POST', '/v1/families', {'family_id': family, 'device_id': owner, 'recovery_verifier': verifier})
assert status == 200, (status, created)
owner_token = created['device_token']
path = f'/v1/families/{family}/events'
assert call('GET', path)[0] == 401

def envelope(origin):
    return {'event_id': str(uuid.uuid4()), 'aggregate_id': str(uuid.uuid4()), 'origin_device_id': origin,
            'key_version': 1, 'nonce': b64(secrets.token_bytes(12)),
            'ciphertext': b64(secrets.token_bytes(48)), 'mac': b64(secrets.token_bytes(16))}

events = [envelope(owner) for _ in range(2)]
def upload(event):
    return call('POST', path, {'events': [event]}, owner_token)
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    results = list(pool.map(upload, events))
assert all(result[0] == 200 for result in results), results
status, page = call('GET', path + '?after=0', token=owner_token)
assert status == 200 and [e['event_id'] for e in page['events']] == [e['event_id'] for e in sorted(page['events'], key=lambda e: e['sequence'])]
assert set(e['event_id'] for e in page['events']) == set(e['event_id'] for e in events)
assert call('POST', path, {'events': [events[0]]}, owner_token)[0] == 200
changed = dict(events[0]); changed['ciphertext'] = b64(secrets.token_bytes(48))
assert call('POST', path, {'events': [changed]}, owner_token)[0] == 409
assert call('GET', path + '?after=' + str(page['events'][-1]['sequence']), token=owner_token)[1]['events'] == []

invite, secret = str(uuid.uuid4()), secrets.token_bytes(32)
inv_path = f'/v1/families/{family}/invites/{invite}'
assert call('POST', f'/v1/families/{family}/invites', {'invite_id': invite, 'verifier': b64(hashlib.sha256(secret).digest())}, owner_token)[0] == 200
proof = {'verifier': b64(hashlib.sha256(secret).digest()), 'device_id': wife}
assert call('POST', f'/v1/invites/{invite}/request', proof)[0] == 200
assert call('GET', inv_path, token=owner_token)[1]['pending_device_id'] == wife
package = {'device_id': wife, 'package_nonce': b64(secrets.token_bytes(12)), 'package_ciphertext': b64(secrets.token_bytes(32)), 'package_mac': b64(secrets.token_bytes(16))}
assert call('POST', inv_path + '/approve', package, owner_token)[0] == 200
wife_recovery = b64(hashlib.sha256(secrets.token_bytes(32)).digest())
claim_proof = {**proof, 'recovery_verifier': wife_recovery}
status, claim = call('POST', f'/v1/invites/{invite}/claim', claim_proof)
assert status == 200, (status, claim)
assert call('POST', f'/v1/invites/{invite}/claim', claim_proof)[0] == 410
wife_token = claim['device_token']
assert len(call('GET', path + '?after=0', token=wife_token)[1]['events']) == 2
assert call('POST', path, {'events': [envelope(wife)]}, wife_token)[0] == 200
owner_recovery = call('POST', f'/v1/families/{family}/recover', {'device_id': str(uuid.uuid4()), 'recovery_verifier': verifier})
wife_recovery_result = call('POST', f'/v1/families/{family}/recover', {'device_id': str(uuid.uuid4()), 'recovery_verifier': wife_recovery})
assert owner_recovery[0] == 200 and owner_recovery[1]['owner'] is True
assert wife_recovery_result[0] == 200 and wife_recovery_result[1]['owner'] is False
print('PASS: auth, concurrent append, cursor, idempotency, one-use join, second-device read/write, role-preserving recovery')
