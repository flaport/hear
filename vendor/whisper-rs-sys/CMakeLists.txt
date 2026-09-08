cmake_minimum_required(VERSION 3.14)
project(hear-whisper LANGUAGES C CXX)
# Build as a subproject so upstream never generates files in the source tree.
add_subdirectory(whisper.cpp)
