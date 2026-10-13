#!/usr/bin/env python3
"""Validate baked inputs without printing key bytes, URLs or other values."""
import os
import re
import sys
from urllib.parse import urlsplit

def validate(env):
    expected={'ONGROW_WINDOWS_UPDATER':'v1','ONGROW_UPDATE_SESSION_GATE':'v1','ONGROW_PRODUCT_ROLE':'customer-desk'}
    if any(env.get(k)!=v for k,v in expected.items()): raise ValueError('Missing Windows updater product policy')
    if not re.fullmatch('[1-9][0-9]*',env.get('ONGROW_RELEASE_SEQUENCE','')) or int(env['ONGROW_RELEASE_SEQUENCE'])>2**32-1: raise ValueError('Invalid release sequence')
    if not re.fullmatch('[0-9a-f]{40}',env.get('ONGROW_RELEASE_SOURCE_SHA','')): raise ValueError('Invalid source revision')
    key=env.get('ONGROW_UPDATE_PUBLIC_KEY_HEX','')
    if not re.fullmatch('[0-9a-f]{64}',key) or key=='0'*64: raise ValueError('Invalid release public key')
    origin=env.get('ONGROW_UPDATE_ORIGIN','')
    url=urlsplit(origin)
    if url.scheme!='https' or not url.hostname or url.username or url.password or url.path or url.query or url.fragment or not origin.isascii() or any(c in origin for c in '%\\?@#') or origin.endswith(':443'): raise ValueError('Invalid update origin')
    prefix=env.get('ONGROW_UPDATE_PREFIX','')
    if not prefix.startswith('/') or not prefix.endswith('/') or not all(re.fullmatch('[A-Za-z0-9_.-]+',p) and p not in ('.','..') for p in prefix[1:-1].split('/')): raise ValueError('Invalid update path')

if __name__=='__main__':
    try: validate(os.environ)
    except (ValueError,TypeError):
        print('Windows updater baked policy rejected',file=sys.stderr);sys.exit(1)
    print('Windows updater baked policy valid')
