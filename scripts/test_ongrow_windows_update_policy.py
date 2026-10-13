#!/usr/bin/env python3
import unittest
from check_ongrow_windows_update_policy import validate

class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.policy=dict(ONGROW_WINDOWS_UPDATER='v1',ONGROW_UPDATE_SESSION_GATE='v1',ONGROW_PRODUCT_ROLE='customer-desk',
            ONGROW_RELEASE_SEQUENCE='19',ONGROW_RELEASE_SOURCE_SHA='a'*40,ONGROW_UPDATE_PUBLIC_KEY_HEX='ab'*32,
            ONGROW_UPDATE_ORIGIN='https://updates.example.com',ONGROW_UPDATE_PREFIX='/desk/windows/lab/')
    def test_valid_complete_compiled_policy(self): validate(self.policy)
    def test_missing_identity_key_sequence_and_endpoint_are_rejected(self):
        for key in self.policy:
            with self.subTest(key=key):
                policy=dict(self.policy);policy.pop(key)
                with self.assertRaises(ValueError):validate(policy)
    def test_untrusted_locations_roles_and_sequences_are_rejected(self):
        for key, values in {
            'ONGROW_PRODUCT_ROLE':['support-console','customer'],
            'ONGROW_RELEASE_SEQUENCE':['0','-1','4294967296','01'],
            'ONGROW_UPDATE_PUBLIC_KEY_HEX':['0'*64,'aa'*31,'GG'*32],
            'ONGROW_UPDATE_ORIGIN':['http://updates.example.com','https://updates.example.com/','https://user@updates.example.com','https://updates.example.com:443','https://updates.example.com?x=1'],
            'ONGROW_UPDATE_PREFIX':['/','/desk/../','/desk/%2f/','/desk//','/desk','//desk/'],
        }.items():
            for value in values:
                with self.subTest(key=key,value=value):
                    policy=dict(self.policy);policy[key]=value
                    with self.assertRaises(ValueError):validate(policy)
if __name__=='__main__':unittest.main()
