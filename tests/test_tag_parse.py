#!/usr/bin/env python3
"""Unit tests for the folder/file-name parsing in scripts/zpod_tag_push.py (no mutagen needed)."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'scripts'))
import zpod_tag_push as z

def eq(a, b):
    assert a == b, f'{a!r} != {b!r}'

eq(z.parse_album_dir('(1958) Miles Davis - Milestones'), ('1958', 'Miles Davis - Milestones'))
eq(z.parse_album_dir('(1949-1951) Miles Davis - Conception'), ('1949', 'Miles Davis - Conception'))
eq(z.parse_album_dir('(1961) Miles Davis - At Carnegie Hall [320 kbps]'), ('1961', 'Miles Davis - At Carnegie Hall'))
eq(z.parse_album_dir('Kind of Blue (1959)'), ('1959', 'Kind of Blue'))
eq(z.parse_album_dir('1959 - Kind of Blue'), ('1959', 'Kind of Blue'))
eq(z.parse_album_dir('1958 Miles'), (None, '1958 Miles'))
eq(z.parse_album_dir('Synchronicity [FLAC]'), (None, 'Synchronicity'))
eq(z.split_artist_album('Miles Davis - Milestones'), ('Miles Davis', 'Milestones'))
eq(z.split_artist_album('Miles Davis - Milestones', 'miles davis'), ('Miles Davis', 'Milestones'))
eq(z.split_artist_album('Tune Up - When Lights', 'Miles Davis'), (None, 'Tune Up - When Lights'))
eq(z.clean_album("Somethin' Else (Cannonball Adderley album)"), "Somethin' Else")
eq(z.artist_from_dir('Miles Davis - FLAC'), 'Miles Davis')
eq(z.artist_from_dir('Compilations'), None)
eq(z.artist_from_dir('to file'), None)
eq(z.artist_from_dir('The Police'), 'The Police')
eq(z.parse_track_file('05 Dr. Jackle'), (None, 5, 'Dr. Jackle'))
eq(z.parse_track_file('01 - So What'), (None, 1, 'So What'))
eq(z.parse_track_file('01. So What'), (None, 1, 'So What'))
eq(z.parse_track_file('1-05 Freddie Freeloader'), (1, 5, 'Freddie Freeloader'))
eq(z.parse_track_file('2-11 Blue in Green'), (2, 11, 'Blue in Green'))
eq(z.parse_track_file('So What'), (None, None, 'So What'))
eq(z.parse_track_file("10 Straight, No Chaser"), (None, 10, 'Straight, No Chaser'))
eq(z.genre_from_path('/Volumes/Media/Jazz/Miles Davis/Kind of Blue'), 'Jazz')
eq(z.genre_from_path('/Volumes/Media/Music/to file/Miles'), None)
assert z.DISC_DIR.match('Disc - 1') and z.DISC_DIR.match('CD2') and not z.DISC_DIR.match('Discipline')
# cover scoring: an exact title (edition notes ignored) + artist is "high"; a different album is not
c = {'collectionName': "Relaxin' With the Miles Davis Quintet (Remastered)", 'artistName': 'Miles Davis Quintet', 'trackCount': 6}
w = {'collectionName': "Workin' With The Miles Davis Quintet", 'artistName': 'Miles Davis Quintet', 'trackCount': 8}
sc, conf, _ = z.score_candidate(c, 'Miles Davis', "Relaxin' with The Miles Davis Quintet", 6)
sw, confw, _ = z.score_candidate(w, 'Miles Davis', "Relaxin' with The Miles Davis Quintet", 6)
assert conf == 'high' and confw != 'high' and sc > sw, (sc, conf, sw, confw)
print('test_tag_parse: all ok')
