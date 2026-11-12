import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    'verify_published_inputs', Path(__file__).resolve().parents[1] / 'scripts/verify-published-inputs.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InputsTest(unittest.TestCase):
    def test_valid_coordinates(self):
        for version in ('0.1.0', '1.2.3-rc.1', '12.0.0-alpha-1'):
            module.validate(version, 'a' * 40)

    def test_invalid_versions(self):
        for version in ('v1.2.3', '01.2.3', '1.2.3-01', '1.2.3+',
                        '1.2.3;echo bad', '1.2.3\n', '1.2.3-'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                module.validate(version, 'a' * 40)

    def test_invalid_sha(self):
        for sha in ('a' * 39, 'A' * 40, 'g' * 40, 'a' * 40 + '\n'):
            with self.subTest(sha=sha), self.assertRaises(ValueError):
                module.validate('1.2.3', sha)
