ERL          ?= erl

EBIN_DIRS    := $(wildcard deps/*/ebin)
ERLC_FLAGS := -W $(INCLUDE_DIRS:%=-I %) $(EBIN_DIRS:%=-pa %)
APP          := embryosys

all: erl ebin/$(APP).app

erl:
	@$(ERL) -pa $(EBIN_DIRS) -noinput +B \
	  -eval 'case make:all() of up_to_date -> halt(0); error -> halt(1) end.'

docs:
	@$(ERL) -noshell -run edoc_run application '$(APP)' '"."' '[{preprocess, true},{includes, ["."]}]'

test: all
	prove t/*.t

cover: all
	COVER=1 prove t/*.t
	erl -detached -noshell -eval 'etap_report:create()' -s init stop

clean: 
	@echo "removing:"
	@rm -fv ebin/*.beam ebin/*.app

ebin/$(APP).app: src/$(APP).app
	@cp -v src/$(APP).app $@


