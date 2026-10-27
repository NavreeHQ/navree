"""Regenerates the fixtures of the AWS SNS and Google Pub/Sub tests.

Run from this directory with OpenSSL 3 on the PATH:

    python -I generate.py

It writes a fresh RSA 2048 signing key (`signer-key.pem`, PKCS#8), a
self-signed certificate for it (`signer-cert.pem`, the stand-in for an SNS
signing certificate) and `sns-messages.json`: SNS messages signed by
OpenSSL, with signature version 1 (SHA-1) and 2 (SHA-256), the way SNS signs
them. The Rust side never signs SHA-1, so these are its independent vectors.
"""

import base64
import json
import os
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KEY = os.path.join(HERE, "signer-key.pem")
CERT = os.path.join(HERE, "signer-cert.pem")

TOPIC = "arn:aws:sns:us-east-1:123456789012:ses-events"
CERT_URL = (
    "https://sns.us-east-1.amazonaws.com/"
    "SimpleNotificationService-9c6465fa7f48f5cacd23014631ec1136.pem"
)
TOKEN = "2336412f37fb687f5d51e6e241d09c805a5a57b30d712f794cc5f6a988666d92"
SUBSCRIBE_URL = (
    "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription"
    f"&TopicArn={TOPIC}&Token={TOKEN}"
)
UNSUBSCRIBE_URL = (
    "https://sns.us-east-1.amazonaws.com/?Action=Unsubscribe"
    f"&SubscriptionArn={TOPIC}:9a7f3b2c-0d1e-4f5a-8b6c-7d8e9f0a1b2c"
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


def sign(message, version):
    digest = "-sha1" if version == "1" else "-sha256"
    with tempfile.TemporaryDirectory() as scratch:
        path = os.path.join(scratch, "message.txt")
        with open(path, "wb") as file:
            file.write(string_to_sign(message).encode())
        signature = subprocess.run(
            ["openssl", "dgst", digest, "-sign", KEY, path],
            check=True,
            capture_output=True,
        ).stdout
    signed = dict(message)
    signed["SignatureVersion"] = version
    signed["Signature"] = base64.b64encode(signature).decode()
    signed["SigningCertURL"] = CERT_URL
    return signed


def main():
    run("openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", KEY)
    run(
        "openssl", "req", "-x509", "-key", KEY, "-subj", "/CN=sns.us-east-1.amazonaws.com",
        "-days", "36500", "-sha256", "-out", CERT,
    )
    ses_bounce = json.dumps(
        {
            "eventType": "Bounce",
            "bounce": {"bounceType": "Permanent", "bouncedRecipients": [{"emailAddress": "ada@example.com"}]},
            "mail": {"messageId": "0100018c-9a7f-4f0e-b2a1-000000000000"},
        },
        separators=(",", ":"),
    )
    notification = {
        "Type": "Notification",
        "MessageId": "da41e39f-ea4d-435a-b922-c6aae3915ebe",
        "TopicArn": TOPIC,
        "Subject": "Amazon SES Email Event Notification",
        "Message": ses_bounce,
        "Timestamp": "2026-10-26T07:59:30.000Z",
        "UnsubscribeURL": UNSUBSCRIBE_URL,
    }
    no_subject = {key: value for key, value in notification.items() if key != "Subject"}
    no_subject["MessageId"] = "f3c5b0c1-6a8e-4c43-9d2c-0e1b7d1f2a3b"
    confirmation = {
        "Type": "SubscriptionConfirmation",
        "MessageId": "165545c9-2a5c-472c-8df2-7ff2be2b3b1b",
        "Token": TOKEN,
        "TopicArn": TOPIC,
        "Message": f"You have chosen to subscribe to the topic {TOPIC}.\n"
        "To confirm the subscription, visit the SubscribeURL included in this message.",
        "SubscribeURL": SUBSCRIBE_URL,
        "Timestamp": "2026-10-26T07:59:00.000Z",
    }
    unsubscribe = dict(confirmation)
    unsubscribe["Type"] = "UnsubscribeConfirmation"
    unsubscribe["MessageId"] = "47138184-6831-46b8-8f7c-afc488602d7d"
    unsubscribe["Message"] = f"You have chosen to deactivate subscription to {TOPIC}."
    fixtures = {
        "notification_v1": sign(notification, "1"),
        "notification_v2": sign(notification, "2"),
        "notification_without_subject_v1": sign(no_subject, "1"),
        "subscription_confirmation_v1": sign(confirmation, "1"),
        "subscription_confirmation_v2": sign(confirmation, "2"),
        "unsubscribe_confirmation_v2": sign(unsubscribe, "2"),
    }
    with open(os.path.join(HERE, "sns-messages.json"), "w", newline="\n") as file:
        json.dump(fixtures, file, indent=2)
        file.write("\n")


if __name__ == "__main__":
    main()
