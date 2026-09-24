# encoding: utf-8

###########################################################################################################
#
#
# All Your BASE
#
# Adds a BASE table with script-specific MinMax values to exported fonts,
# measured by autobase (https://github.com/simoncozens/autobase) through a bundled Rust library.
#
#
###########################################################################################################

from __future__ import division, print_function, unicode_literals
import ctypes
import json
import os
import re
import objc
from GlyphsApp import Glyphs, DOCUMENTEXPORTED
from GlyphsApp.plugins import GeneralPlugin


enableParameterName = "BASE Table"
toleranceParameterName = "BASE Tolerance"
languagesParameterName = "BASE Languages"
overrideParameterName = "BASE Override"
exclusionsParameterName = "BASE Exclusions"
wordsPerListParameterName = "BASE Words Per List"
wordsFileParameterName = "BASE Words File"
ideographicBottomParameterName = "BASE Ideographic Bottom"
defaultWordsPerList = 1000

# yyy_Xxxx or Xxxx: optional ISO 639 language, ISO 15924 script
scriptLanguagePattern = re.compile(r"^(?:([a-z]{2,3})_)?([A-Z][a-z]{3})$")
supportedExtensions = (".otf", ".ttf")

_library = None


def library():
	global _library
	if _library is None:
		path = os.path.join(os.path.dirname(__file__), "libautobase_glyphs.dylib")
		_library = ctypes.CDLL(path)
		_library.autobase_glyphs_process.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
		_library.autobase_glyphs_process.restype = ctypes.c_void_p
		_library.autobase_glyphs_free.argtypes = [ctypes.c_void_p]
		_library.autobase_glyphs_free.restype = None
	return _library


def runAutobase(fontPath, request):
	"""
	Measures the font, writes the BASE table into it and returns the response dict.
	"""
	lib = library()
	pointer = lib.autobase_glyphs_process(fontPath.encode("utf-8"), json.dumps(request).encode("utf-8"))
	try:
		return json.loads(ctypes.string_at(pointer).decode("utf-8"))
	finally:
		lib.autobase_glyphs_free(pointer)


def isOn(value):
	if value is True or value == 1:
		return True
	return str(value).strip().lower() in ("1", "true", "yes", "on")


def listFromCode(code):
	return [particle.strip() for particle in str(code).split(",") if particle.strip()]


def parseOverride(code):
	"""
	fi_Latn; max=1234 → ("fi_Latn", {"max": 1234}), []
	"""
	key, _, rest = str(code).partition(";")
	key = key.strip()
	if not scriptLanguagePattern.match(key):
		return None, None, ["‘%s’: ‘%s’ is not a script (Latn) or language_script (fi_Latn) code." % (code, key)]
	values = {}
	for particle in listFromCode(rest):
		name, _, number = particle.partition("=")
		name = name.strip()
		if name not in ("min", "max"):
			return None, None, ["‘%s’: expected min=… and/or max=…, found ‘%s’." % (code, particle)]
		try:
			values[name] = int(number.strip())
		except ValueError:
			return None, None, ["‘%s’: ‘%s’ is not a whole number." % (code, number.strip())]
	if not values:
		return None, None, ["‘%s’: expected min=… and/or max=… after the semicolon." % code]
	return key, values, []


def parseWordsFile(path):
	"""
	Reads a UTF-8 file with [Xxxx] or [yyy_Xxxx] section headers followed by
	whitespace-separated words. Lines starting with # are comments.
	"""
	extraWords = []
	problems = []
	current = None
	with open(path, encoding="utf-8") as wordsFile:
		for lineNumber, line in enumerate(wordsFile, start=1):
			line = line.strip()
			if not line or line.startswith("#"):
				continue
			if line.startswith("[") and line.endswith("]"):
				match = scriptLanguagePattern.match(line[1:-1].strip())
				if not match:
					problems.append("%s line %i: ‘%s’ is not a [Latn] or [fi_Latn] section header." % (os.path.basename(path), lineNumber, line))
					current = None
					continue
				current = {"script": match.group(2), "language": match.group(1), "words": []}
				extraWords.append(current)
			elif current is None:
				problems.append("%s line %i: words before the first section header." % (os.path.basename(path), lineNumber))
			else:
				current["words"].extend(line.split())
	return [entry for entry in extraWords if entry["words"]], problems


def requestForInstance(instance):
	"""
	Returns (request, problems). The request is None if the plug-in is not switched on for this instance.
	"""
	parameters = [parameter for parameter in instance.customParameters if parameter.active]
	if not any(parameter.name == enableParameterName and isOn(parameter.value) for parameter in parameters):
		return None, []

	config = {"override": {}, "languages": [], "tolerance": None, "exclusions": []}
	wordsPerList = defaultWordsPerList
	ideographicBottom = None
	extraWords = []
	problems = []
	for parameter in parameters:
		name, value = parameter.name, parameter.value
		if name == toleranceParameterName or name == wordsPerListParameterName:
			try:
				number = int(str(value).strip())
				if number < 0:
					raise ValueError
			except ValueError:
				problems.append("%s: ‘%s’ is not a positive whole number." % (name, value))
				continue
			if name == toleranceParameterName:
				config["tolerance"] = number
			else:
				wordsPerList = number
		elif name == ideographicBottomParameterName:
			try:
				ideographicBottom = int(str(value).strip())
			except ValueError:
				problems.append("%s: ‘%s’ is not a whole number." % (name, value))
		elif name == languagesParameterName:
			for code in listFromCode(value):
				if scriptLanguagePattern.match(code) and "_" in code:
					config["languages"].append(code)
				else:
					problems.append("%s: ‘%s’ is not a language_script code like fi_Latn." % (name, code))
		elif name == overrideParameterName:
			key, values, overrideProblems = parseOverride(value)
			problems.extend("%s: %s" % (name, problem) for problem in overrideProblems)
			if key:
				config["override"].setdefault(key, {}).update(values)
		elif name == exclusionsParameterName:
			config["exclusions"].extend(listFromCode(value))
		elif name == wordsFileParameterName:
			path = os.path.expanduser(str(value).strip())
			if not os.path.isabs(path):
				fontPath = instance.font.filepath if instance.font else None
				if not fontPath:
					problems.append("%s: ‘%s’ is relative, but the font has not been saved yet." % (name, value))
					continue
				path = os.path.join(os.path.dirname(fontPath), path)
			if not os.path.isfile(path):
				problems.append("%s: file not found: %s" % (name, path))
				continue
			fileWords, fileProblems = parseWordsFile(path)
			extraWords.extend(fileWords)
			problems.extend("%s: %s" % (name, problem) for problem in fileProblems)

	request = {"autobase": config, "words_per_list": wordsPerList, "extra_words": extraWords, "ideographic_bottom": ideographicBottom}
	return request, problems


def reportProblems(problems, instanceName):
	print("⚠️ All Your BASE: %i problem%s in the BASE parameters of ‘%s’:" % (
		len(problems),
		"" if len(problems) == 1 else "s",
		instanceName,
	))
	for problem in problems:
		print("   • %s" % problem)
	print("   No BASE table was added. Fix the parameters in Font Info → Exports and export again.")
	Glyphs.showMacroWindow()


class AllYourBASE(GeneralPlugin):

	@objc.python_method
	def settings(self):
		self.name = Glyphs.localize({
			'en': 'All Your BASE',
		})

	@objc.python_method
	def start(self):
		Glyphs.addCallback(self.fontsExported_, DOCUMENTEXPORTED)

	def fontsExported_(self, info):
		exportInfo = info.object()
		instance = exportInfo["instance"]
		request, problems = requestForInstance(instance)
		if problems:
			reportProblems(problems, instance.name)
			return False, None
		if request is None:
			return False, None

		fontPaths = exportInfo.get("fontFilePaths") or [exportInfo["fontFilePath"]]
		showMacroWindow = False
		for fontPath in fontPaths:
			fontName = os.path.basename(fontPath)
			if not fontPath.lower().endswith(supportedExtensions):
				print("All Your BASE: skipped %s (only OTF and TTF are supported)." % fontName)
				continue
			response = runAutobase(fontPath, request)
			if response["error"]:
				print("❌ All Your BASE: %s: %s" % (fontName, response["error"]))
				showMacroWindow = True
			elif not response["written"]:
				print("All Your BASE: %s: no supported scripts found, no BASE table added." % fontName)
			else:
				print("✅ All Your BASE: %s: %s BASE table:" % (fontName, "replaced existing" if response["replaced_existing"] else "added"))
				for line in response["summary"]:
					print("   %s" % line)
		if showMacroWindow:
			Glyphs.showMacroWindow()
		return True, "BASE table added."

	@objc.python_method
	def __del__(self):
		Glyphs.removeCallback(self.fontsExported_, DOCUMENTEXPORTED)

	@objc.python_method
	def __file__(self):
		"""Please leave this method unchanged"""
		return __file__
