#!/usr/bin/env python3

import os
import optparse
import subprocess
from hashlib import md5
import datetime

# 4GB maximum
length_count = 4
# encoding
encoding = 'utf-8'

# output: {path: (compressed_data, file_md5)}


def generate_md5_table(folder: str, level) -> dict:
    import brotli
    res: dict = dict()
    for root, directories, files in os.walk(folder):
        directories.sort()
        for f in sorted(files):
            md5_generator = md5()
            full_path = os.path.join(root, f)
            print(f"Processing {full_path}...")
            with open(full_path, "rb") as source:
                content = source.read()
            content_compressed = brotli.compress(
                content, quality=level)
            md5_generator.update(content)
            md5_code = md5_generator.hexdigest().encode(encoding=encoding)
            relative = './' + os.path.relpath(full_path, folder).replace(os.sep, '/')
            res[relative] = (content_compressed, md5_code)
    return res


def write_package_metadata(md5_table: dict, output_folder: str, exe: str):
    output_path = os.path.join(output_folder, "data.bin")
    with open(output_path, "wb") as f:
        f.write("rustdesk".encode(encoding=encoding))
        for path in md5_table.keys():
            (compressed_data, md5_code) = md5_table[path]
            data_length = len(compressed_data)
            path = path.encode(encoding=encoding)
            # path length & path
            f.write((len(path)).to_bytes(length=length_count, byteorder='big'))
            f.write(path)
            # data length & compressed data
            f.write(data_length.to_bytes(
                length=length_count, byteorder='big'))
            f.write(compressed_data)
            # md5 code
            f.write(md5_code)
        # end
        f.write("rustdesk".encode(encoding=encoding))
        # executable
        f.write(exe.encode(encoding='utf-8'))
    print(f"Metadata has been written to {output_path}")

def write_app_metadata(output_folder: str):
    output_path = os.path.join(output_folder, "app_metadata.toml")
    with open(output_path, "w") as f:
        f.write(f"timestamp = {int(datetime.datetime.now().timestamp() * 1000)}\n")
    print(f"App metadata has been written to {output_path}")

def build_portable(output_folder: str, target: str, feature: str = None):
    current_dir = os.getcwd()
    try:
        os.chdir(output_folder)
        cmd = ["cargo", "build", "--locked", "--release"]
        if target:
            cmd.extend(["--target", target])
        if feature:
            cmd.extend(["--features", feature])
        subprocess.run(cmd, check=True)
    finally:
        os.chdir(current_dir)

# Linux: python3 generate.py -f ../rustdesk-portable-packer/test -o . -e ./test/main.py
# Windows: python3 .\generate.py -f ..\rustdesk\flutter\build\windows\runner\Debug\ -o . -e ..\rustdesk\flutter\build\windows\runner\Debug\rustdesk.exe


if __name__ == '__main__':
    parser = optparse.OptionParser()
    parser.add_option("-f", "--folder", dest="folder",
                      help="folder to compress")
    parser.add_option("-o", "--output", dest="output_folder",
                      help="the root of portable packer project, default is './'")
    parser.add_option("-e", "--executable", dest="executable",
                      help="specify startup file in --folder, default is rustdesk.exe")
    parser.add_option("-t", "--target", dest="target",
                      help="the target used by cargo")
    parser.add_option("--feature", dest="feature", help="optional cargo feature")
    parser.add_option("-l", "--level", dest="level", type="int",
                      help="compression level, default is 11, highest", default=11)
    (options, args) = parser.parse_args()
    folder = options.folder or './rustdesk'
    output_folder = os.path.abspath(options.output_folder or './')

    if not options.executable:
        options.executable = 'rustdesk.exe'
    exe = os.path.abspath(options.executable if os.path.isabs(options.executable) or os.path.isfile(options.executable)
                          else os.path.join(folder, options.executable))
    source = os.path.abspath(folder)
    if os.path.commonpath([source, exe]) != source or not os.path.isfile(exe):
        print("The executable must locate in source folder")
        exit(-1)
    exe = './' + os.path.relpath(exe, source).replace(os.sep, '/')
    print("Executable path: " + exe)
    print("Compression level: " + str(options.level))
    md5_table = generate_md5_table(folder, options.level)
    write_package_metadata(md5_table, output_folder, exe)
    write_app_metadata(output_folder)
    build_portable(output_folder, options.target, options.feature)
