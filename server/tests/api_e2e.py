"""Black-box API tests against a running Tackly server and PostgreSQL.

Run after migrations with TACKLY_TEST_BASE_URL set if the server is not on
http://127.0.0.1:3000. Every test creates fresh families and devices.
"""

import base64
import concurrent.futures
import hashlib
import json
import os
import secrets
import shlex
import subprocess
import unittest
import urllib.error
import urllib.request
import uuid


BASE = os.environ.get("TACKLY_TEST_BASE_URL", "http://127.0.0.1:3000").rstrip("/")


def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def identifier():
    return str(uuid.uuid4())


def verifier():
    return b64(hashlib.sha256(secrets.token_bytes(32)).digest())


def request(method, path, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers["Authorization"] = "Bearer " + token
    payload = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=payload, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        raw = error.read()
        error.close()
        try:
            body = json.loads(raw)
        except ValueError:
            body = {"error": raw.decode(errors="replace")}
        return error.code, body


def family():
    family_id, device_id, recovery = identifier(), identifier(), verifier()
    status, result = request(
        "POST", "/v1/families",
        {"family_id": family_id, "device_id": device_id, "recovery_verifier": recovery},
    )
    assert status == 200, (status, result)
    return family_id, device_id, recovery, result["device_token"]


def event(origin, **changes):
    envelope = {
        "event_id": identifier(),
        "aggregate_id": identifier(),
        "origin_device_id": origin,
        "key_version": 1,
        "nonce": b64(secrets.token_bytes(12)),
        "ciphertext": b64(secrets.token_bytes(48)),
        "mac": b64(secrets.token_bytes(16)),
    }
    envelope.update(changes)
    return envelope


def invite(family_id, token):
    invite_id, secret = identifier(), secrets.token_bytes(32)
    proof = b64(hashlib.sha256(secret).digest())
    path = f"/v1/families/{family_id}/invites/{invite_id}"
    status, result = request(
        "POST", f"/v1/families/{family_id}/invites",
        {"invite_id": invite_id, "verifier": proof}, token,
    )
    assert status == 200, (status, result)
    assert result["expires_at_utc"]
    return invite_id, proof, path


class ApiEndToEnd(unittest.TestCase):
    def test_family_creation_recovery_and_isolation(self):
        self.assertEqual(request("GET", "/health")[0], 204)
        family_id, _, recovery, owner_token = family()
        other_family, _, _, other_token = family()
        events_path = f"/v1/families/{family_id}/events"
        self.assertEqual(request("GET", events_path)[0], 401)
        self.assertEqual(request("GET", events_path, token="wrong")[0], 401)
        self.assertEqual(request("GET", events_path, token=other_token)[0], 401)
        self.assertEqual(request("GET", f"/v1/families/{other_family}/events", token=owner_token)[0], 401)
        self.assertEqual(request(
            "POST", "/v1/families",
            {"family_id": family_id, "device_id": identifier(), "recovery_verifier": verifier()},
        )[0], 409)
        self.assertEqual(request(
            "POST", "/v1/families",
            {"family_id": identifier(), "device_id": identifier(), "recovery_verifier": "short"},
        )[0], 400)
        recover_path = f"/v1/families/{family_id}/recover"
        self.assertEqual(request(
            "POST", recover_path,
            {"device_id": identifier(), "recovery_verifier": verifier()},
        )[0], 401)
        status, recovered = request(
            "POST", recover_path,
            {"device_id": identifier(), "recovery_verifier": recovery},
        )
        self.assertEqual(status, 200)
        self.assertTrue(recovered["owner"])
        self.assertEqual(request("GET", events_path, token=recovered["device_token"])[0], 200)
        self.assertNotEqual(recovered["device_token"], owner_token)

    def test_encrypted_events_validation_paging_and_idempotency(self):
        family_id, origin, _, token = family()
        path = f"/v1/families/{family_id}/events"
        upload = lambda batch: request("POST", path, {"events": batch}, token)
        self.assertEqual(upload([])[0], 400)
        self.assertEqual(upload([event(origin) for _ in range(9)])[0], 400)
        for invalid in [
            event(identifier()),
            event(origin, key_version=0),
            event(origin, nonce="bad"),
            event(origin, mac=b64(b"short")),
            event(origin, ciphertext=""),
            event(origin, ciphertext=b64(b"x" * (2 * 1024 * 1024 + 1))),
        ]:
            self.assertEqual(upload([invalid])[0], 400)
        self.assertEqual(request("GET", path, token=token)[1]["events"], [])
        first = event(origin)
        self.assertEqual(upload([first, event(origin, key_version=0)])[0], 400)
        self.assertEqual(request("GET", path, token=token)[1]["events"], [])
        second, third = event(origin), event(origin)
        self.assertEqual(upload([first, second, third])[0], 200)
        self.assertEqual(upload([first])[0], 200)
        changed = {**first, "ciphertext": b64(secrets.token_bytes(48))}
        self.assertEqual(upload([changed])[0], 409)
        status, first_page = request("GET", path + "?after=0&limit=2", token=token)
        self.assertEqual(status, 200)
        self.assertEqual([row["event_id"] for row in first_page["events"]], [first["event_id"], second["event_id"]])
        self.assertEqual(first_page["events"][0]["ciphertext"], first["ciphertext"])
        cursor = first_page["events"][-1]["sequence"]
        tail = request("GET", path + f"?after={cursor}&limit=2", token=token)[1]["events"]
        self.assertEqual([row["event_id"] for row in tail], [third["event_id"]])
        self.assertEqual(request("GET", path + f"?after={tail[-1]['sequence']}", token=token)[1]["events"], [])
        self.assertEqual(request("GET", path + "?after=-1", token=token)[0], 400)
        self.assertEqual(request("GET", path + "?limit=21", token=token)[0], 400)

    def test_invitation_join_permissions_and_member_recovery(self):
        family_id, origin, _, owner_token = family()
        invite_id, proof, path = invite(family_id, owner_token)
        member_id = identifier()
        request_path = f"/v1/invites/{invite_id}/request"
        claim_path = f"/v1/invites/{invite_id}/claim"
        self.assertEqual(request("POST", request_path, {"verifier": verifier(), "device_id": member_id})[0], 410)
        self.assertEqual(request("POST", claim_path, {
            "verifier": proof, "device_id": member_id, "recovery_verifier": verifier(),
        })[0], 410)
        self.assertEqual(request("POST", request_path, {"verifier": proof, "device_id": member_id})[0], 200)
        self.assertEqual(request("POST", request_path, {"verifier": proof, "device_id": identifier()})[0], 410)
        self.assertEqual(request("GET", path, token=owner_token)[1]["pending_device_id"], member_id)
        package = {
            "device_id": member_id,
            "package_nonce": b64(secrets.token_bytes(12)),
            "package_ciphertext": b64(secrets.token_bytes(32)),
            "package_mac": b64(secrets.token_bytes(16)),
        }
        self.assertEqual(request("POST", path + "/approve", {**package, "device_id": identifier()}, owner_token)[0], 410)
        self.assertEqual(request("POST", path + "/approve", package, owner_token)[0], 200)
        member_recovery = verifier()
        claim_input = {"verifier": proof, "device_id": member_id, "recovery_verifier": member_recovery}
        self.assertEqual(request("POST", claim_path, {**claim_input, "device_id": identifier()})[0], 410)
        status, claimed = request("POST", claim_path, claim_input)
        self.assertEqual(status, 200)
        self.assertEqual(claimed["family_id"], family_id)
        for field in ("package_nonce", "package_ciphertext", "package_mac"):
            self.assertEqual(claimed[field], package[field])
        self.assertEqual(request("POST", claim_path, claim_input)[0], 410)
        member_token = claimed["device_token"]
        events_path = f"/v1/families/{family_id}/events"
        self.assertEqual(request("POST", events_path, {"events": [event(origin)]}, owner_token)[0], 200)
        self.assertEqual(len(request("GET", events_path, token=member_token)[1]["events"]), 1)
        self.assertEqual(request("POST", events_path, {"events": [event(member_id)]}, member_token)[0], 200)
        self.assertEqual(request("POST", events_path, {"events": [event(origin)]}, member_token)[0], 400)
        self.assertEqual(request("POST", f"/v1/families/{family_id}/invites", {
            "invite_id": identifier(), "verifier": verifier(),
        }, member_token)[0], 403)
        self.assertEqual(request("GET", path, token=member_token)[0], 403)
        self.assertEqual(request("DELETE", path, token=member_token)[0], 403)
        recovered = request("POST", f"/v1/families/{family_id}/recover", {
            "device_id": identifier(), "recovery_verifier": member_recovery,
        })[1]
        self.assertFalse(recovered["owner"])
        self.assertEqual(request("GET", events_path, token=recovered["device_token"])[0], 200)

    def test_cancellation_prevents_join(self):
        family_id, _, _, owner_token = family()
        for stage in ("open", "pending", "approved"):
            invite_id, proof, path = invite(family_id, owner_token)
            member_id = identifier()
            if stage != "open":
                self.assertEqual(request("POST", f"/v1/invites/{invite_id}/request", {
                    "verifier": proof, "device_id": member_id,
                })[0], 200)
            if stage == "approved":
                package = {
                    "device_id": member_id,
                    "package_nonce": b64(secrets.token_bytes(12)),
                    "package_ciphertext": b64(secrets.token_bytes(32)),
                    "package_mac": b64(secrets.token_bytes(16)),
                }
                self.assertEqual(request("POST", path + "/approve", package, owner_token)[0], 200)
            self.assertEqual(request("DELETE", path, token=owner_token)[1]["status"], "cancelled")
            self.assertEqual(request("GET", path, token=owner_token)[1]["status"], "cancelled")
            self.assertEqual(request("POST", f"/v1/invites/{invite_id}/request", {
                "verifier": proof, "device_id": member_id,
            })[0], 410)
            self.assertEqual(request("POST", f"/v1/invites/{invite_id}/claim", {
                "verifier": proof, "device_id": member_id, "recovery_verifier": verifier(),
            })[0], 410)

    @unittest.skipUnless(os.environ.get("TACKLY_TEST_DB_COMMAND"), "requires disposable database SQL command")
    def test_expired_invitation_cannot_be_used(self):
        family_id, _, _, owner_token = family()
        invite_id, proof, path = invite(family_id, owner_token)
        command = shlex.split(os.environ["TACKLY_TEST_DB_COMMAND"])
        if os.environ.get("DATABASE_URL") and command[0] == "psql":
            command.append(os.environ["DATABASE_URL"])
        subprocess.run(
            command + [
                "-v", "ON_ERROR_STOP=1", "-c",
                f"UPDATE invitations SET expires_at=now()-interval '1 second' WHERE id='{invite_id}'",
            ],
            check=True,
            capture_output=True,
        )
        self.assertEqual(request("GET", path, token=owner_token)[0], 410)
        self.assertEqual(request("POST", f"/v1/invites/{invite_id}/request", {
            "verifier": proof, "device_id": identifier(),
        })[0], 410)

    def test_concurrent_append_never_skips_a_committed_event(self):
        family_id, origin, _, token = family()
        path = f"/v1/families/{family_id}/events"
        envelopes = [event(origin) for _ in range(8)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            results = list(executor.map(
                lambda item: request("POST", path, {"events": [item]}, token), envelopes,
            ))
        self.assertTrue(all(status == 200 for status, _ in results), results)
        cursor, seen = 0, set()
        while True:
            status, page = request("GET", path + f"?after={cursor}&limit=1", token=token)
            self.assertEqual(status, 200)
            if not page["events"]:
                break
            item = page["events"][0]
            self.assertGreater(item["sequence"], cursor)
            cursor = item["sequence"]
            seen.add(item["event_id"])
        self.assertEqual(seen, {item["event_id"] for item in envelopes})


if __name__ == "__main__":
    unittest.main(verbosity=2)
