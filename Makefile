ERL          ?= erl

EBIN_DIRS    := $(wildcard deps/*/ebin)
ERLC_FLAGS   := -W $(INCLUDE_DIRS:%=-I %) $(EBIN_DIRS:%=-pa %)
APP          := embryosys

all:
	./rebar compile

doc: all
	##@mkdir -p doc
	##@$(ERL) -noshell -run edoc_run application '$(APP)' '"."' '[{preprocess, true},{includes, ["."]}]'
	(cd deps/orange; make doc)
	(cd deps/similar; make doc)

test: all
	#prove t/*.t
	(cd deps/orange; make test)
	(cd deps/similar; make test)

cover: all
	##COVER=1 prove t/*.t
	##erl -detached -noshell -eval 'etap_report:create()' -s init stop
	(cd deps/orange; make cover)
	(cd deps/similar; make cover)

clean: 
	./rebar clean


