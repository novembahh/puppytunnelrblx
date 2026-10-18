cmake_minimum_required(VERSION 3.16)
project(studio_textmode C)
set(CMAKE_C_STANDARD 99)

add_library(textmode SHARED src/textmode.c)
set_target_properties(textmode PROPERTIES PREFIX "" OUTPUT_NAME "textmode")

add_executable(tm_inject src/tm_inject.c)
