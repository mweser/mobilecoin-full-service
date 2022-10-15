# get value in from output, then decode
import os
import sys
from decimal import Decimal
import zlib
import base64
import json

import external_pb2

args = sys.argv


def parse_b64_receipt(b64_string):
    """Convert a b64-encoded protobuf Receipt into a full-service receipt object"""
    receipt_bytes = base64.b64decode(b64_string)
    receipt = external_pb2.Receipt.FromString(receipt_bytes)

    print(receipt)

    full_service_receipt = {
        "object": "receiver_receipt",
        "public_key": receipt.public_key.SerializeToString().hex(),
        "confirmation": receipt.confirmation.SerializeToString().hex(),
        "tombstone_block": str(int(receipt.tombstone_block)),
        "amount": {
            "object": "amount",
            "commitment": receipt.amount.commitment.data.hex(),
            "masked_value": str(int(receipt.amount.masked_value)),
            "masked_token_id": str(int(receipt.amount.masked_token_id)),
        },
    }

    return full_service_receipt


print(parse_b64_receipt(args[1]))
