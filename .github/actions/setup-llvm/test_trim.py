import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('trim_llvm', Path(__file__).with_name('trim-llvm.py'))
llvm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(llvm)


class TrimTests(unittest.TestCase):
    def test_keeps_compiler_resources_and_runtime_libraries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'keysteer-llvm'
            retained = [f'bin/{tool}' for tool in llvm.TOOLS] + [
                'lib/libLLVM.so.23', 'lib/clang/23/include/stddef.h',
                'lib/clang/23/lib/linux/libclang_rt.builtins.a', 'LICENSE.TXT']
            removed = ['bin/lldb', 'lib/libLLVMCore.a', 'include/llvm/Module.h',
                       'share/doc/LLVM/manual.txt']
            for name in retained + removed:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'fixture')
            with patch.dict(os.environ, RUNNER_TEMP=temp):
                llvm.trim(root)
            for name in retained:
                self.assertTrue((root / name).is_file(), name)
            for name in removed:
                self.assertFalse((root / name).exists(), name)

    def test_rejects_other_directory_without_deleting(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'keep.txt'
            path.write_text('keep')
            with patch.dict(os.environ, RUNNER_TEMP=temp):
                with self.assertRaises(ValueError):
                    llvm.trim(Path(temp))
            self.assertEqual(path.read_text(), 'keep')


if __name__ == '__main__':
    unittest.main()
