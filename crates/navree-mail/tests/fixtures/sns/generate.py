"""Regenerates the signed SNS fixtures of the SES route tests.

Run from this directory with OpenSSL 3 on the PATH:

    python -I generate.py

It writes a fresh RSA 2048 signing key (`signer-key.pem`, PKCS#8) and a
self-signed certificate for it (`signer-cert.pem`, the stand-in for an SNS
signing certificate), signs the two SES receipt notifications of
`../inbound` in place, and writes `sns-messages.json`: SES event
notifications and subscription confirmations signed by OpenSSL with
signature version 1 (SHA-1) and 2 (SHA-256), the way SNS signs them, and
one notification signed by another key, whose certificate is
`other-cert.pem`. The Rust side never signs SHA-1, so these are its
independent vectors.
"""

import base64
import json
import os
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
INBOUND = os.path.join(HERE, "..", "inbound")
KEY = os.path.join(HERE, "signer-key.pem")
CERT = os.path.join(HERE, "signer-cert.pem")
OTHER_CERT = os.path.join(HERE, "other-cert.pem")

EVENTS_TOPIC = "arn:aws:sns:eu-west-1:123456789012:ses"
INBOUND_TOPIC = "arn:aws:sns:eu-west-1:123456789012:inbound-mail"
CERT_URL = (
    "https://sns.eu-west-1.amazonaws.com/"
    "SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem"
)

NOTIFICATION_KEYS = ["Message", "MessageId", "Subject", "Timestamp", "TopicArn", "Type"]
CONFIRMATION_KEYS = [
    "Message",
    "MessageId",
    "SubscribeURL",
    "Timestamp",
    "Token",
    "TopicArn",
    "Type",
]


def run(*args):
    subprocess.run(args, check=True, cwd=HERE)


def string_to_sign(message):
    keys = NOTIFICATION_KEYS if message["Type"] == "Notification" else CONFIRMATION_KEYS
    return "".join(f"{key}\n{message[key]}\n" for key in keys if key in message)


def sign(message, version, key=KEY):
    digest = "-sha1" if version == "1" else "-sha256"
    with tempfile.TemporaryDirectory() as scratch:
        path = os.path.join(scratch, "message.txt")
        with open(path, "wb") as file:
            file.write(string_to_sign(message).encode())
        signature = subprocess.run(
            ["openssl", "dgst", digest, "-sign", key, path],
            check=True,
            capture_output=True,
        ).stdout
    signed = dict(message)
    signed["SignatureVersion"] = version
    signed["Signature"] = base64.b64encode(signature).decode()
    signed["SigningCertURL"] = CERT_URL
    return signed


def lf(path):
    """Rewrites `path` with LF line endings (OpenSSL writes CRLF on Windows)."""
    with open(path, "rb") as file:
        data = file.read()
    with open(path, "wb") as file:
        file.write(data.replace(b"\r\n", b"\n"))


def write_json(path, value):
    with open(path, "w", newline="\n") as file:
        json.dump(value, file, indent=2)
        file.write("\n")


def confirmation(topic, message_id, token, timestamp):
    return {
        "Type": "SubscriptionConfirmation",
        "MessageId": message_id,
        "Token": token,
        "TopicArn": topic,
        "Message": f"You have chosen to subscribe to the topic {topic}.\n"
        "To confirm the subscription, visit the SubscribeURL included in this message.",
        "SubscribeURL": "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription"
        f"&TopicArn={topic}&Token={token}",
        "Timestamp": timestamp,
    }


def main():
    run("openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", KEY)
    run(
        "openssl", "req", "-x509", "-key", KEY, "-subj", "/CN=sns.eu-west-1.amazonaws.com",
        "-days", "36500", "-sha256", "-out", CERT,
    )
    lf(KEY)
    lf(CERT)

    # The receipt notifications of the inbound route, signed in place.
    for name, version in [("ses-sns-action.json", "1"), ("ses-s3-action.json", "2")]:
        path = os.path.join(INBOUND, name)
        with open(path) as file:
            message = json.load(file)
        write_json(path, sign(message, version))

    # An SES event: the bounce of the first message the mail fake sends.
    bounce = json.dumps(
        {
            "eventType": "Bounce",
            "mail": {
                "messageId": "fake-1",
                "timestamp": "2026-10-27T10:00:00.000Z",
                "destination": ["ada@example.com"],
            },
            "bounce": {
                "bounceType": "Permanent",
                "bounceSubType": "General",
                "bouncedRecipients": [
                    {
                        "emailAddress": "ada@example.com",
                        "diagnosticCode": "smtp; 550 5.1.1 user unknown",
                    }
                ],
                "timestamp": "2026-10-27T10:00:01.000Z",
            },
        },
        separators=(",", ":"),
    )
    notification = {
        "Type": "Notification",
        "MessageId": "9e1b6f0a-3c2d-4e5f-8a7b-1c2d3e4f5a6b",
        "TopicArn": EVENTS_TOPIC,
        "Subject": "Amazon SES Email Event Notification",
        "Message": bounce,
        "Timestamp": "2026-10-27T10:00:02.000Z",
        "UnsubscribeURL": "https://sns.eu-west-1.amazonaws.com/?Action=Unsubscribe"
        f"&SubscriptionArn={EVENTS_TOPIC}:4b9d2c1e-7f6a-4e3d-9c8b-2a1f0e9d8c7b",
    }
    with tempfile.TemporaryDirectory() as scratch:
        other_key = os.path.join(scratch, "other-key.pem")
        subprocess.run(
            ["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048",
             "-out", other_key],
            check=True,
        )
        subprocess.run(
            ["openssl", "req", "-x509", "-key", other_key, "-subj", "/CN=sns.eu-west-1.amazonaws.com",
             "-days", "36500", "-sha256", "-out", OTHER_CERT],
            check=True,
        )
        lf(OTHER_CERT)
        other = sign(notification, "2", other_key)
    unsubscribe = confirmation(
        EVENTS_TOPIC,
        "2c4e6a8b-0d1f-4a3c-9e5b-7d9f1b3d5f7a",
        "6f4a1c9e2b7d5f3a8c0e4b6d2f8a1c3e5b7d9f0a2c4e6b8d0f1a3c5e7b9d2f4a",
        "2026-10-27T10:00:03.000Z",
    )
    unsubscribe["Type"] = "UnsubscribeConfirmation"
    unsubscribe["Message"] = f"You have chosen to deactivate subscription to {EVENTS_TOPIC}."
    fixtures = {
        "ses_bounce_v1": sign(notification, "1"),
        "ses_bounce_v2": sign(notification, "2"),
        "ses_bounce_other_key_v2": other,
        "ses_subscription_confirmation_v1": sign(
            confirmation(
                EVENTS_TOPIC,
                "7a3f9c2e-1b4d-4f6a-8c0e-2d4f6a8c0e1b",
                "2336412f37fb687f5d51e6e241d09c805a5a57b30d712f794cc5f6a988666d92",
                "2026-10-27T09:59:00.000Z",
            ),
            "1",
        ),
        "ses_unsubscribe_confirmation_v2": sign(unsubscribe, "2"),
        # A bounce on the inbound topic: signed, but no received mail.
        "inbound_bounce_v2": sign(
            {
                "Type": "Notification",
                "MessageId": "5e7a9c1b-3d5f-4a7c-9e1b-3d5f7a9c1e3a",
                "TopicArn": INBOUND_TOPIC,
                "Subject": "Amazon SES Email Receipt Notification",
                "Message": json.dumps({"notificationType": "Bounce"}, separators=(",", ":")),
                "Timestamp": "2026-10-27T09:14:10.000Z",
            },
            "2",
        ),
        "inbound_subscription_confirmation_v2": sign(
            confirmation(
                INBOUND_TOPIC,
                "3d5f7a9c-1e3a-4c5e-8b7d-9f1b3d5f7a9c",
                "9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b",
                "2026-10-27T09:14:00.000Z",
            ),
            "2",
        ),
    }
    write_json(os.path.join(HERE, "sns-messages.json"), fixtures)


if __name__ == "__main__":
    main()
